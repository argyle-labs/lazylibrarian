#![allow(clippy::disallowed_types)]

use plugin_toolkit::service::{
    BoxFuture, Routes, Runtime, ServiceBackend, ServiceCapability, ServiceError, ServiceStatus,
    WorkloadSpec,
};

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

    fn data_paths(&self) -> Vec<String> {
        vec!["/config".to_string()]
    }

    fn workload_spec<'a>(
        &'a self,
        _runtime: Runtime,
        _instance: &'a str,
        _routes: &'a Routes,
    ) -> BoxFuture<'a, Result<WorkloadSpec, ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.workload_spec")) })
    }

    fn configure<'a>(
        &'a self,
        _instance: &'a str,
        _routes: &'a Routes,
        _config: &'a str,
    ) -> BoxFuture<'a, Result<(), ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.configure")) })
    }

    fn status<'a>(
        &'a self,
        _instance: &'a str,
        _routes: &'a Routes,
    ) -> BoxFuture<'a, Result<ServiceStatus, ServiceError>> {
        Box::pin(async move { Err(ServiceError::unimplemented("lazylibrarian.status")) })
    }
}

pub mod api;
pub mod tools;

use plugin_toolkit::clap;
use plugin_toolkit::media::{
    Capability, MediaBackend, MediaError, MediaItem, MediaMutation, MediaType,
};

#[plugin_toolkit::endpoint_resource(plugin = "lazylibrarian")]
pub struct LazylibrarianEndpoint {
    pub name: String,
    #[secret]
    pub api_key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct LlMedia {
    media_type: MediaType,
}

impl LlMedia {
    pub fn acquires(media_type: MediaType) -> Self {
        Self { media_type }
    }

    pub fn ll_type(&self) -> &'static str {
        match self.media_type {
            MediaType::Audiobooks => "AudioBook",
            _ => "eBook",
        }
    }

    async fn connect(&self) -> Result<api::Ll, MediaError> {
        let rows = endpoint_db::list()
            .map_err(|e| MediaError::Transport(format!("read endpoints: {e}")))?;
        let ep = rows
            .into_iter()
            .filter(|r| r.enabled)
            .min_by(|a, b| a.name.cmp(&b.name))
            .ok_or_else(|| MediaError::NotFound("no enabled lazylibrarian endpoint".into()))?;
        tools::connect(&ep.name).await.map_err(transport)
    }

    fn to_item(&self, b: api::Book) -> MediaItem {
        let status = match self.media_type {
            MediaType::Audiobooks => b.audio_status,
            _ => b.ebook_status,
        };
        MediaItem {
            id: b.id,
            title: b.title,
            status,
            path: None,
        }
    }
}

fn transport(e: plugin_toolkit::anyhow::Error) -> MediaError {
    MediaError::Transport(format!("{e:#}"))
}

#[plugin_toolkit::orca_async]
impl MediaBackend for LlMedia {
    fn name(&self) -> &str {
        "lazylibrarian"
    }
    fn media_type(&self) -> MediaType {
        self.media_type
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::DownloadedBy,
            Capability::List,
            Capability::Search,
            Capability::LibraryAdd,
        ]
    }
    fn endpoint(&self) -> String {
        String::new()
    }

    async fn list(&self) -> Result<Vec<MediaItem>, MediaError> {
        let books = self.connect().await?.books(&[]).await.map_err(transport)?;
        Ok(books.into_iter().map(|b| self.to_item(b)).collect())
    }

    async fn search(&self, query: &str) -> Result<Vec<MediaItem>, MediaError> {
        let books = self
            .connect()
            .await?
            .find_books(query)
            .await
            .map_err(transport)?;
        Ok(books.into_iter().map(|b| self.to_item(b)).collect())
    }

    async fn library_add(&self, item_ref: &str) -> Result<MediaMutation, MediaError> {
        let ty = self.ll_type();
        self.connect()
            .await?
            .queue_book(item_ref, ty)
            .await
            .map_err(transport)?;
        Ok(MediaMutation {
            ok: true,
            message: Some(format!("queueBook id={item_ref} type={ty}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_toolkit::serde_json::{self, json};
    use std::sync::{Arc, Mutex};

    pub(crate) const KEY: &str = "0123456789abcdef0123456789abcdef";

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
        assert_eq!(m.ll_type(), "eBook");
    }

    pub(crate) fn serve<F>(
        route: impl Fn(&str) -> Option<(u16, String)> + 'static,
        body: F,
    ) -> Vec<String>
    where
        F: FnOnce(),
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |cap: &str, req: &str| {
                assert_eq!(cap, "http.request");
                let req: serde_json::Value = serde_json::from_str(req).unwrap();
                assert_eq!(req["method"], "GET");
                let url = req["url"].as_str().unwrap().to_string();
                let query = url
                    .strip_prefix("http://ll.test/api?")
                    .unwrap_or_else(|| panic!("unexpected url {url}"))
                    .to_string();
                assert!(query.contains(&format!("apikey={KEY}")), "{query}");
                let query = query.replace(&format!("apikey={KEY}&"), "");
                log.lock().unwrap().push(query.clone());
                let (status, body) = route(&query).unwrap_or((404, "Not Found".to_string()));
                Ok(json!({"status": status, "headers": [], "body": body.into_bytes()}).to_string())
            }),
            body,
        );
        let out = seen.lock().unwrap().clone();
        out
    }

    pub(crate) fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    pub(crate) fn ll() -> api::Ll {
        api::Ll::new("http://ll.test/", KEY)
    }

    #[test]
    fn books_read_getallbooks_with_apikey() {
        let mut got = Vec::new();
        let calls = serve(
            |q| {
                (q == "cmd=getAllBooks").then(|| {
                    (
                        200,
                        json!([{"BookID": "GB1", "BookName": "Elantris", "Status": "Open", "audiostatus": "Skipped"}])
                            .to_string(),
                    )
                })
            },
            || got = block(ll().books(&[])).unwrap(),
        );
        assert_eq!(calls, vec!["cmd=getAllBooks"]);
        assert_eq!(got[0].id, "GB1");
        let audio = LlMedia::acquires(MediaType::Audiobooks).to_item(got[0].clone());
        assert_eq!(audio.status.as_deref(), Some("Skipped"));
    }

    #[test]
    fn find_book_percent_encodes_the_name() {
        let calls = serve(
            |_| {
                Some((
                    200,
                    json!([{"bookid": "GB9", "bookname": "The Way of Kings"}]).to_string(),
                ))
            },
            || {
                let b = block(ll().find_books("The Way of Kings & more")).unwrap();
                assert_eq!(b[0].id, "GB9");
            },
        );
        assert_eq!(calls.len(), 1);
        assert!(calls[0].starts_with("cmd=findBook&name=The"), "{calls:?}");
        assert!(!calls[0].contains(" & "), "{calls:?}");
    }

    #[test]
    fn queue_book_sends_id_and_type() {
        let calls = serve(
            |_| Some((200, "OK".to_string())),
            || block(ll().queue_book("GB1", "AudioBook")).unwrap(),
        );
        assert_eq!(calls, vec!["cmd=queueBook&id=GB1&type=AudioBook"]);
    }

    #[test]
    fn a_command_fails_unless_upstream_answers_ok() {
        serve(
            |_| Some((200, "Invalid id: GB404".to_string())),
            || {
                let e = block(ll().queue_book("GB404", "eBook")).unwrap_err();
                assert!(e.to_string().contains("Invalid id"), "{e}");
            },
        );
    }

    #[test]
    fn api_refusals_in_a_200_surface_as_errors() {
        serve(
            |_| {
                Some((
                    200,
                    json!({"Success": false, "Data": "", "Error": {"Code": 405, "Message": "Command: queueBook not available with read-only api access key"}})
                        .to_string(),
                ))
            },
            || {
                let e = block(ll().books(&[])).unwrap_err().to_string();
                assert!(e.contains("read-only"), "{e}");
            },
        );
    }

    #[test]
    fn error_bodies_are_truncated_scrubbed_and_keyless() {
        serve(
            |_| {
                Some((
                    500,
                    format!(
                        r#"{{"apikey":"{KEY}","password":"hunter2hunter2","pad":"{}"}}"#,
                        "x".repeat(2000)
                    ),
                ))
            },
            || {
                let e = block(ll().books(&[])).unwrap_err().to_string();
                assert!(e.contains("HTTP 500"), "{e}");
                assert!(e.len() < 400, "{}", e.len());
                assert!(!e.contains(KEY), "{e}");
                assert!(!e.contains("hunter2hunter2"), "{e}");
            },
        );
    }

    #[test]
    fn connect_fails_when_the_api_key_secret_is_missing() {
        let caps = Arc::new(Mutex::new(Vec::new()));
        let log = caps.clone();
        let e = plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |cap: &str, _: &str| {
                log.lock().unwrap().push(cap.to_string());
                match cap {
                    "secret.op" => Ok(json!({"value": null}).to_string()),
                    _ => Err(format!("unexpected {cap}")),
                }
            }),
            || block(tools::connect("ll")).err().unwrap(),
        );
        assert!(format!("{e:#}").contains("lazylibrarian"), "{e:#}");
        assert_eq!(*caps.lock().unwrap(), vec!["secret.op"]);
    }
}
