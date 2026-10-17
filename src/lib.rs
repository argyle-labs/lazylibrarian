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
pub mod parity;
pub mod tools;

use plugin_toolkit::clap;
use plugin_toolkit::media::{
    Capability, MediaBackend, MediaError, MediaIdentity, MediaItem, MediaMutation, MediaType,
    MediaUnit,
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
            Capability::Units,
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

    async fn units(&self) -> Result<Vec<MediaUnit>, MediaError> {
        let books = self.connect().await?.books(&[]).await.map_err(transport)?;
        Ok(units(books, self.media_type))
    }
}

pub fn units(books: Vec<api::Book>, media_type: MediaType) -> Vec<MediaUnit> {
    books
        .into_iter()
        .filter_map(|b| to_unit(b, media_type))
        .collect()
}

pub fn to_unit(b: api::Book, media_type: MediaType) -> Option<MediaUnit> {
    let status = match media_type {
        MediaType::Audiobooks => b.audio_status.as_deref(),
        MediaType::Ebooks => b.ebook_status.as_deref(),
        _ => return None,
    };
    if b.title.is_empty() || parity::availability(status) != parity::Availability::Have {
        return None;
    }
    let mut identity = MediaIdentity {
        title: b.title,
        year: b
            .published
            .as_deref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok()),
        external_ids: Vec::new(),
        series: None,
    };
    if let Some(isbn) = &b.isbn {
        identity = identity.with_external_id("isbn", isbn, media_type);
    }
    Some(MediaUnit {
        media_type,
        identity,
        variants: Vec::new(),
        sources: Vec::new(),
        downloaded_by: vec!["lazylibrarian".to_string()],
        served_by: Vec::new(),
    })
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
            Capability::Units,
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

    fn ok_route(q: &str) -> Option<(u16, String)> {
        let _ = q;
        Some((200, "OK".to_string()))
    }

    #[test]
    fn authors_read_getindex() {
        let calls = serve(
            |_| {
                Some((
                    200,
                    json!([{"AuthorID": "A1", "AuthorName": "N", "HaveBooks": 1}]).to_string(),
                ))
            },
            || assert_eq!(block(ll().authors()).unwrap()[0].id, "A1"),
        );
        assert_eq!(calls, vec!["cmd=getIndex"]);
    }

    #[test]
    fn books_list_filters_server_side_and_by_author() {
        let calls = serve(
            |_| {
                Some((
                    200,
                    json!([
                        {"BookID": "B1", "AuthorID": "A1", "Status": "Wanted"},
                        {"BookID": "B2", "AuthorID": "A2", "Status": "Wanted"}
                    ])
                    .to_string(),
                ))
            },
            || {
                let args = tools::BooksArgs {
                    name: "ll".into(),
                    status: Some("Wanted".into()),
                    audio_status: Some("Skipped".into()),
                    author_id: Some("A1".into()),
                    limit: Some(50),
                };
                let got = block(tools::list_books(&ll(), &args)).unwrap();
                assert_eq!(got.len(), 1);
                assert_eq!(got[0].id, "B1");
            },
        );
        assert_eq!(
            calls,
            vec!["cmd=getAllBooks&status=Wanted&audiostatus=Skipped&limit=50"]
        );
    }

    #[test]
    fn want_both_queues_each_format_and_reports_failures() {
        let calls = serve(
            |q| {
                if q.contains("id=BAD&type=AudioBook") {
                    Some((200, "Invalid id: BAD".to_string()))
                } else {
                    ok_route(q)
                }
            },
            || {
                let args = tools::WantArgs {
                    name: "ll".into(),
                    book_ids: vec!["B1".into(), "BAD".into()],
                    format: tools::Format::Both,
                };
                let r = block(tools::want_books(&ll(), &args)).unwrap();
                let got: Vec<(&str, tools::Format, bool)> = r
                    .results
                    .iter()
                    .map(|x| (x.id.as_str(), x.format, x.wanted))
                    .collect();
                assert_eq!(
                    got,
                    vec![
                        ("B1", tools::Format::Ebook, true),
                        ("B1", tools::Format::Audiobook, true),
                        ("BAD", tools::Format::Ebook, true),
                        ("BAD", tools::Format::Audiobook, false),
                    ]
                );
                assert!(r.results[3]
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("Invalid id"));
            },
        );
        assert_eq!(calls.len(), 4);
    }

    #[test]
    fn want_rejects_bad_ids_before_any_call() {
        let calls = serve(ok_route, || {
            let args = tools::WantArgs {
                name: "ll".into(),
                book_ids: vec!["B1".into(), "x&cmd=shutdown".into()],
                format: tools::Format::Ebook,
            };
            assert!(block(tools::want_books(&ll(), &args)).is_err());
        });
        assert!(calls.is_empty());
    }

    #[test]
    fn wanted_search_passes_type_only_for_one_format() {
        let calls = serve(ok_route, || {
            assert!(
                block(tools::search_wanted(&ll(), tools::Format::Both))
                    .unwrap()
                    .started
            );
            block(tools::search_wanted(&ll(), tools::Format::Audiobook)).unwrap();
        });
        assert_eq!(
            calls,
            vec!["cmd=forceBookSearch", "cmd=forceBookSearch&type=AudioBook"]
        );
    }

    #[test]
    fn wanted_search_fails_when_no_search_method_is_configured() {
        serve(
            |_| Some((200, "No search methods set, check config".to_string())),
            || {
                let e = block(tools::search_wanted(&ll(), tools::Format::Both)).unwrap_err();
                assert!(e.to_string().contains("No search methods"), "{e}");
            },
        );
    }

    #[test]
    fn scan_both_runs_ebook_and_audiobook_scans() {
        let calls = serve(ok_route, || {
            let args = tools::ScanArgs {
                name: "ll".into(),
                format: tools::Format::Both,
                author_id: Some("A1".into()),
            };
            let r = block(tools::scan_library(&ll(), &args)).unwrap();
            assert_eq!(
                r.started,
                vec![tools::Format::Ebook, tools::Format::Audiobook]
            );
        });
        assert_eq!(
            calls,
            vec!["cmd=forceLibraryScan&id=A1", "cmd=forceAudioBookScan&id=A1"]
        );
    }

    fn scan_args() -> tools::ScanArgs {
        tools::ScanArgs {
            name: "ll".into(),
            format: tools::Format::Both,
            author_id: None,
        }
    }

    #[test]
    fn scan_both_reports_a_partial_start() {
        serve(
            |q| {
                if q.starts_with("cmd=forceAudioBookScan") {
                    Some((500, "boom".to_string()))
                } else {
                    ok_route(q)
                }
            },
            || {
                let r = block(tools::scan_library(&ll(), &scan_args())).unwrap();
                assert_eq!(r.started, vec![tools::Format::Ebook]);
                assert_eq!(r.failed[0].id, "audiobook");
                assert!(r.failed[0].error.contains("HTTP 500"), "{:?}", r.failed);
            },
        );
    }

    #[test]
    fn scan_fails_when_no_format_starts() {
        serve(
            |_| Some((500, "boom".to_string())),
            || assert!(block(tools::scan_library(&ll(), &scan_args())).is_err()),
        );
    }

    #[test]
    fn transport_errors_carrying_the_url_are_redacted() {
        let e = plugin_toolkit::capsink::with_cap_sink(
            Box::new(|_: &str, _: &str| {
                Err(format!(
                    "error sending request for url (http://ll.test/api?apikey={KEY}&cmd=getIndex)"
                ))
            }),
            || block(ll().authors()).unwrap_err().to_string(),
        );
        assert!(e.starts_with("getIndex:"), "{e}");
        assert!(!e.contains(KEY), "{e}");
        assert!(e.contains("apikey=***"), "{e}");
    }

    #[test]
    fn dry_run_writes_make_no_http_request() {
        let cfg = std::sync::Arc::new(plugin_toolkit::contract::config::Config::load().unwrap());
        let ctx = plugin_toolkit::contract::ToolCtx::new(cfg);
        let caps = Arc::new(Mutex::new(Vec::new()));
        let log = caps.clone();
        plugin_toolkit::capsink::with_cap_sink(
            Box::new(move |cap: &str, _: &str| {
                log.lock().unwrap().push(cap.to_string());
                Err(format!("unexpected {cap}"))
            }),
            || {
                for (tool, args) in [
                    (
                        "lazylibrarian.books.want",
                        json!({"name": "ll", "book_ids": ["B1"], "format": "both", "dryRun": true}),
                    ),
                    (
                        "lazylibrarian.wanted.search",
                        json!({"name": "ll", "dryRun": true}),
                    ),
                    (
                        "lazylibrarian.library.scan",
                        json!({"name": "ll", "dryRun": true}),
                    ),
                ] {
                    let out = block(plugin_toolkit::dispatch::dispatch(tool, args, &ctx))
                        .unwrap_or_else(|e| panic!("{tool}: {e:#}"));
                    assert_eq!(out["dryRun"], json!(true), "{tool}: {out}");
                }
            },
        );
        let caps = caps.lock().unwrap();
        assert!(!caps.iter().any(|c| c == "http.request"), "{caps:?}");
    }

    #[test]
    fn units_carry_isbn_and_only_owned_format() {
        let mut got = Vec::new();
        serve(
            |_| {
                Some((
                    200,
                    json!([
                        {"BookID": "B1", "BookName": "The Way of Kings", "BookIsbn": "0-7653-2635-5",
                         "BookDate": "2010-08-31", "Status": "Open", "audiostatus": "Wanted"},
                        {"BookID": "B2", "BookName": "Elantris", "Status": "Skipped", "audiostatus": "Open"}
                    ])
                    .to_string(),
                ))
            },
            || {
                let books = block(ll().books(&[])).unwrap();
                got = units(books.clone(), MediaType::Ebooks);
                assert_eq!(
                    units(books.clone(), MediaType::Audiobooks)[0]
                        .identity
                        .title,
                    "Elantris"
                );
                assert!(units(books, MediaType::Movies).is_empty());
            },
        );
        assert_eq!(got.len(), 1);
        let u = &got[0];
        assert_eq!(u.identity.year, Some(2010));
        assert_eq!(u.downloaded_by, vec!["lazylibrarian"]);
        assert!(u
            .identity
            .external_ids
            .iter()
            .any(|e| e.source == "isbn" && e.id == "9780765326355"));
    }
}
