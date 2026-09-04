//! lazylibrarian service backend — LazyLibrarian ebook/audiobook library manager.
//!
//! Implements `ServiceBackend` so the generic `service.*` tools
//! (deploy/backup/restore/configure/status/connect/sync) drive lazylibrarian. No
//! `#[orca_tool]`s — the only orca dep is `plugin-toolkit`. Modeled on the
//! nfs StorageBackend. See orca/docs/PLUGIN-PROGRAM.md.
#![allow(clippy::disallowed_types)]

use plugin_toolkit::service::{
    BoxFuture, Endpoint, Runtime, ServiceBackend, ServiceCapability, ServiceError, ServiceStatus,
    WorkloadSpec,
};

/// lazylibrarian backend. Holds only the provider name; per-instance endpoint/creds
/// come from the `Endpoint` the generic `service.*` tools hand each op.
#[derive(Debug, Clone)]
pub struct LazylibrarianBackend {
    provider: &'static str,
}

impl LazylibrarianBackend {
    pub fn new(provider: &'static str) -> Self {
        Self { provider }
    }
}

impl ServiceBackend for LazylibrarianBackend {
    fn provider(&self) -> &str {
        self.provider
    }

    /// Runtimes lazylibrarian can be placed on. `service.deploy` hands the
    /// `workload_spec` below to a matching deploy target — this backend never
    /// drives pct/docker itself (that mechanic lives in the deploy-target domain).
    fn runtimes(&self) -> Vec<Runtime> {
        vec![Runtime::Docker, Runtime::Podman, Runtime::Lxc]
    }

    fn capabilities(&self) -> Vec<ServiceCapability> {
        vec![
            ServiceCapability::Deploy,
            ServiceCapability::Backup,
            ServiceCapability::Restore,
            ServiceCapability::Configure,
            ServiceCapability::Status,
        ]
    }

    fn default_port(&self) -> u16 {
        5299
    }

    /// In-workload paths holding config/data. This is ALL lazylibrarian declares for
    /// backup — the generic pluggable backup (tar for containers/LXC, PBS for
    /// Proxmox guests when available) snapshots these. No backup/restore code
    /// here; those are inherited from ServiceBackend's defaults.
    fn data_paths(&self) -> Vec<String> {
        vec!["/config".to_string()]
    }

    fn workload_spec<'a>(
        &'a self,
        _runtime: Runtime,
        _ep: &'a Endpoint,
    ) -> BoxFuture<'a, Result<WorkloadSpec, ServiceError>> {
        // TODO: describe the lazylibrarian workload (image/template, ports, mounts,
        // env) for the chosen runtime. The deploy target turns this into a
        // compose service / LXC config / VM. See deploy-target::WorkloadSpec.
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.workload_spec")) })
    }

    fn configure<'a>(
        &'a self,
        _ep: &'a Endpoint,
        _config: &'a str,
    ) -> BoxFuture<'a, Result<(), ServiceError>> {
        // TODO: apply lazylibrarian-specific config idempotently.
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.configure")) })
    }

    fn status<'a>(
        &'a self,
        _ep: &'a Endpoint,
    ) -> BoxFuture<'a, Result<ServiceStatus, ServiceError>> {
        // TODO: real health/diagnostics.
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.status")) })
    }
}

// ── Media downloaded_by facet ────────────────────────────────────────────────
//
// LazyLibrarian is not only a deployable *service* — it *acquires* ebooks and
// audiobooks. It registers a `media` backend per acquired type so orca's generic
// `media downloaded-by` surface drives search / queue (library add) / listing
// from ONE place, instead of per-app. See orca#403 / #404.

use plugin_toolkit::clap; // the endpoint_resource! tools emit unqualified `clap::` paths
use plugin_toolkit::http::Client as HttpClient;
use plugin_toolkit::media::{
    Capability, MediaBackend, MediaError, MediaItem, MediaMutation, MediaType,
};
use plugin_toolkit::{serde, serde_json};

/// Endpoint registry for the LazyLibrarian server orca talks to: `(base_url,
/// api_key)` keyed by `name`. `endpoint_resource!` emits the row struct,
/// `endpoint_db` accessors, schema fragment, and the
/// `lazylibrarian.{list,detail,create,update,delete}` CRUD tools in one shot. The
/// media facet resolves its connection from an enabled row here at call time.
#[plugin_toolkit::endpoint_resource(plugin = "lazylibrarian")]
pub struct LazylibrarianEndpoint {
    pub name: String,
    pub base_url: String,
    #[secret]
    pub api_key: String,
    pub enabled: bool,
}

/// A media `downloaded_by` backend for one media type LazyLibrarian acquires.
/// Registered per acquired type (ebooks, audiobooks) — the builder gives each a
/// type-qualified invoke prefix so they never collide.
#[derive(Debug, Clone)]
pub struct LlMedia {
    media_type: MediaType,
}

impl LlMedia {
    /// Register LazyLibrarian as the acquirer (`downloaded_by`) for `media_type`.
    pub fn acquires(media_type: MediaType) -> Self {
        Self { media_type }
    }

    /// Load the first enabled configured endpoint. The `endpoint_db` read is only
    /// valid inside an `Invoke` (cap sink active), so it's called from the async
    /// verbs, never from `endpoint()`.
    fn resolve_config(&self) -> Result<LlConfig, MediaError> {
        let rows = endpoint_db::list()
            .map_err(|e| MediaError::Transport(format!("read endpoints: {e}")))?;
        let ep = rows
            .into_iter()
            .find(|r| r.enabled)
            .ok_or_else(|| MediaError::NotFound("no enabled lazylibrarian endpoint".into()))?;
        Ok(LlConfig {
            base_url: ep.base_url,
            api_key: ep.api_key,
        })
    }

    /// LazyLibrarian's `queueBook` type token for this media type.
    fn ll_type(&self) -> &'static str {
        match self.media_type {
            MediaType::Audiobooks => "AudioBook",
            _ => "eBook",
        }
    }
}

#[plugin_toolkit::orca_async]
impl MediaBackend for LlMedia {
    fn name(&self) -> &str {
        "lazylibrarian"
    }
    fn media_type(&self) -> MediaType {
        self.media_type
    }
    /// An acquirer: enumerate the tracked library, search providers, and queue a
    /// book for download (library add).
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::DownloadedBy,
            Capability::List,
            Capability::Search,
            Capability::LibraryAdd,
        ]
    }
    /// Built at startup (outside the capability sink), so this must not touch the
    /// db — the real connection is resolved lazily in the verbs.
    fn endpoint(&self) -> String {
        String::new()
    }

    /// Every book LazyLibrarian tracks (`getAllBooks`).
    async fn list(&self) -> Result<Vec<MediaItem>, MediaError> {
        let cfg = self.resolve_config()?;
        let books = LlClient::new(&cfg).get_books("getAllBooks", &[]).await?;
        Ok(books.into_iter().map(LlBook::into_item).collect())
    }

    /// Search Goodreads/GoogleBooks for a title (`findBook&name=`). The returned
    /// ids are what [`library_add`](Self::library_add) queues.
    async fn search(&self, query: &str) -> Result<Vec<MediaItem>, MediaError> {
        let cfg = self.resolve_config()?;
        let books = LlClient::new(&cfg)
            .get_books("findBook", &[("name", query)])
            .await?;
        Ok(books.into_iter().map(LlBook::into_item).collect())
    }

    /// Queue a book for download (`queueBook&id=&type=`), marking it Wanted.
    async fn library_add(&self, item_ref: &str) -> Result<MediaMutation, MediaError> {
        let cfg = self.resolve_config()?;
        let ty = self.ll_type();
        let resp = LlClient::new(&cfg)
            .api("queueBook", &[("id", item_ref), ("type", ty)])
            .await?;
        Ok(MediaMutation {
            ok: (200..300).contains(&resp),
            message: Some(format!("queueBook id={item_ref} type={ty} → HTTP {resp}")),
        })
    }
}

// ── LazyLibrarian HTTP client ────────────────────────────────────────────────

/// Resolved connection to one LazyLibrarian server.
struct LlConfig {
    base_url: String,
    api_key: String,
}

/// Minimal LazyLibrarian API client. Every call is `GET {base}/api?apikey=…&cmd=…`
/// plus command params; JSON commands return a book array (bare or `{data:[…]}`).
struct LlClient<'a> {
    cfg: &'a LlConfig,
    http: HttpClient,
}

impl<'a> LlClient<'a> {
    fn new(cfg: &'a LlConfig) -> Self {
        Self {
            cfg,
            http: HttpClient::new(),
        }
    }

    /// Build the `/api` URL for `cmd` with `params` (apikey + values url-encoded).
    fn url(&self, cmd: &str, params: &[(&str, &str)]) -> String {
        let mut url = format!(
            "{}/api?apikey={}&cmd={}",
            self.cfg.base_url.trim_end_matches('/'),
            percent_encode(&self.cfg.api_key),
            percent_encode(cmd),
        );
        for (k, v) in params {
            url.push('&');
            url.push_str(k);
            url.push('=');
            url.push_str(&percent_encode(v));
        }
        url
    }

    /// Fire a command, returning the HTTP status (for mutations like `queueBook`).
    async fn api(&self, cmd: &str, params: &[(&str, &str)]) -> Result<u16, MediaError> {
        let resp = self
            .http
            .get(self.url(cmd, params))
            .send()
            .await
            .map_err(|e| MediaError::Transport(format!("{cmd}: {e}")))?;
        Ok(resp.status)
    }

    /// Fire a JSON command and decode its book list (bare array or `{data:[…]}`).
    async fn get_books(
        &self,
        cmd: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<LlBook>, MediaError> {
        let resp = self
            .http
            .get(self.url(cmd, params))
            .send()
            .await
            .map_err(|e| MediaError::Transport(format!("{cmd}: {e}")))?;
        let value = resp
            .json::<serde_json::Value>()
            .map_err(|e| MediaError::Transport(format!("decode {cmd}: {e}")))?;
        // LazyLibrarian returns either a bare array or `{ "data": [ … ] }`.
        let arr = value
            .get("data")
            .filter(|d| d.is_array())
            .unwrap_or(&value)
            .clone();
        serde_json::from_value::<Vec<LlBook>>(arr)
            .map_err(|e| MediaError::Transport(format!("decode {cmd} rows: {e}")))
    }
}

/// URL-encode a query value (RFC 3986 unreserved set passes through).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One row of a LazyLibrarian book command. Field casing varies across
/// commands (`getAllBooks` PascalCase vs `findBook` lowercase), so each field
/// carries aliases.
#[derive(serde::Deserialize)]
#[serde(crate = "plugin_toolkit::serde")]
struct LlBook {
    #[serde(default, rename = "BookID", alias = "bookid")]
    book_id: String,
    #[serde(default, rename = "BookName", alias = "bookname", alias = "title")]
    book_name: String,
    #[serde(default, rename = "Status", alias = "status")]
    status: Option<String>,
}

impl LlBook {
    fn into_item(self) -> MediaItem {
        MediaItem {
            id: self.book_id,
            title: self.book_name,
            status: self.status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_provider() {
        let b = LazylibrarianBackend::new("lazylibrarian");
        assert_eq!(b.provider(), "lazylibrarian");
    }

    #[test]
    fn media_facet_is_a_downloaded_by_acquirer() {
        let m = LlMedia::acquires(MediaType::Ebooks);
        assert_eq!(m.name(), "lazylibrarian");
        assert_eq!(m.media_type(), MediaType::Ebooks);
        for cap in [
            Capability::DownloadedBy,
            Capability::List,
            Capability::Search,
            Capability::LibraryAdd,
        ] {
            assert!(m.capabilities().contains(&cap), "missing {cap:?}");
        }
        assert_eq!(
            LlMedia::acquires(MediaType::Audiobooks).ll_type(),
            "AudioBook"
        );
        assert_eq!(LlMedia::acquires(MediaType::Ebooks).ll_type(), "eBook");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(percent_encode("The Way of Kings"), "The%20Way%20of%20Kings");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    }

    // Pins the getAllBooks row shape this backend maps from.
    #[test]
    fn ll_book_decodes_getallbooks_row() {
        let raw = serde_json::json!({
            "BookID": "GB123", "BookName": "The Way of Kings",
            "AuthorName": "Brandon Sanderson", "BookIsbn": "9780765326355", "Status": "Open"
        });
        let b: LlBook = serde_json::from_value(raw).unwrap();
        let item = b.into_item();
        assert_eq!(item.id, "GB123");
        assert_eq!(item.title, "The Way of Kings");
        assert_eq!(item.status.as_deref(), Some("Open"));
    }
}
