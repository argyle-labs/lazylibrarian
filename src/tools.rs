use plugin_toolkit::prelude::*;

use crate::api::{check_id, Author, Book, Ll};
use crate::endpoint_db;
use crate::parity::{self, ParityRow};

pub async fn connect(name: &str) -> Result<Ll> {
    let api_key = plugin_toolkit::secrets::resolve_scoped("lazylibrarian", name, "api_key")?;
    let row = endpoint_db::require(name)?;
    require_routes(name, &row.routes)?;
    let base_url = route::resolve_reachable(name, &row.routes, false).await?;
    Ok(Ll::new(&base_url, api_key.expose_secret()))
}

pub fn require_routes<T>(name: &str, routes: &[T]) -> Result<()> {
    if routes.is_empty() {
        bail!("endpoint {name} has no routes; set routes (base_url is no longer used)");
    }
    Ok(())
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    plugin_toolkit::serde::Serialize,
    plugin_toolkit::serde::Deserialize,
    plugin_toolkit::schemars::JsonSchema,
    plugin_toolkit::clap::ValueEnum,
)]
#[serde(crate = "plugin_toolkit::serde")]
#[schemars(crate = "plugin_toolkit::schemars")]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Ebook,
    Audiobook,
    Both,
}

impl Format {
    pub fn ll_types(self) -> &'static [&'static str] {
        match self {
            Format::Ebook => &["eBook"],
            Format::Audiobook => &["AudioBook"],
            Format::Both => &["eBook", "AudioBook"],
        }
    }

    pub fn singles(self) -> Vec<Format> {
        match self {
            Format::Both => vec![Format::Ebook, Format::Audiobook],
            f => vec![f],
        }
    }
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub id: String,
    pub error: String,
}

#[orca_struct(args)]
pub struct EndpointArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
}

/// List tracked authors with status and have/total book counts (`getIndex`).
#[orca_tool(domain = "lazylibrarian", verb = "authors.list", role = "read")]
async fn lazylibrarian_authors_list(args: EndpointArgs, _ctx: &ToolCtx) -> Result<Vec<Author>> {
    connect(&args.name).await?.authors().await
}

#[orca_struct(args)]
pub struct BooksArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Only books with this ebook status (`Wanted`, `Open`, `Skipped`, ...). At least one of status, audio_status, author_id or limit is required.
    #[arg(long)]
    pub status: Option<String>,
    /// Only books with this audiobook status. At least one of status, audio_status, author_id or limit is required.
    #[arg(long)]
    pub audio_status: Option<String>,
    /// Only books by this author id (`getAuthor`, bounded to that author). At least one of status, audio_status, author_id or limit is required.
    #[arg(long)]
    pub author_id: Option<String>,
    /// Return at most this many books, applied after every filter. At least one of status, audio_status, author_id or limit is required.
    #[arg(long)]
    pub limit: Option<u32>,
}

fn filter_author_books(books: Vec<Book>, args: &BooksArgs) -> Vec<Book> {
    let matches = |want: &Option<String>, have: &Option<String>| {
        want.as_deref()
            .is_none_or(|w| have.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(w)))
    };
    books
        .into_iter()
        .filter(|b| matches(&args.status, &b.ebook_status))
        .filter(|b| matches(&args.audio_status, &b.audio_status))
        .take(args.limit.map_or(usize::MAX, |n| n as usize))
        .collect()
}

pub async fn list_books(ll: &Ll, args: &BooksArgs) -> Result<Vec<Book>> {
    if let Some(id) = &args.author_id {
        return Ok(filter_author_books(ll.author_books(id).await?, args));
    }
    if args.status.is_none() && args.audio_status.is_none() && args.limit.is_none() {
        let statuses: Vec<&str> = parity::STATUSES.iter().map(|(s, _)| *s).collect();
        bail!(
            "pass status (--status), audio_status (--audio-status), author_id (--author-id) or \
             limit (--limit); an unfiltered getAllBooks is unbounded (statuses: {}, Skipped)",
            statuses.join(", ")
        );
    }
    let mut params = Vec::new();
    if let Some(s) = &args.status {
        params.push(("status", s.as_str()));
    }
    if let Some(s) = &args.audio_status {
        params.push(("audiostatus", s.as_str()));
    }
    let limit = args.limit.map(|n| n.to_string());
    if let Some(n) = &limit {
        params.push(("limit", n.as_str()));
    }
    ll.books(&params).await
}

/// List books with ebook and audiobook status, ISBN, author, language and
/// publication date (`getAllBooks`, or `getAuthor` with an author id). Requires a status, audio
/// status, author id or limit.
#[orca_tool(domain = "lazylibrarian", verb = "books.list", role = "read")]
async fn lazylibrarian_books_list(args: BooksArgs, _ctx: &ToolCtx) -> Result<Vec<Book>> {
    list_books(&connect(&args.name).await?, &args).await
}

#[orca_struct(args)]
pub struct ParityArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Include books where either format is `Ignored`.
    #[arg(long)]
    #[serde(default)]
    pub include_ignored: bool,
}

/// List books whose ebook and audiobook availability differ, with which
/// format is missing. Input for the ebook and audiobook parity report.
#[orca_tool(domain = "lazylibrarian", verb = "parity.list", role = "read")]
async fn lazylibrarian_parity_list(args: ParityArgs, _ctx: &ToolCtx) -> Result<Vec<ParityRow>> {
    let books = parity_books(&connect(&args.name).await?, args.include_ignored).await?;
    Ok(parity::parity(books, args.include_ignored))
}

/// A parity gap needs at least one format outside `Skipped`, so querying each
/// such status per format covers every gap without pulling the Skipped/Skipped
/// bulk of a large library.
pub async fn parity_books(ll: &Ll, include_ignored: bool) -> Result<Vec<Book>> {
    let statuses: Vec<&str> = parity::STATUSES
        .iter()
        .filter(|(_, a)| include_ignored || *a != parity::Availability::Ignored)
        .map(|(s, _)| *s)
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut books = Vec::new();
    for key in ["status", "audiostatus"] {
        for status in &statuses {
            for book in ll.books(&[(key, status)]).await? {
                if seen.insert(book.id.clone()) {
                    books.push(book);
                }
            }
        }
    }
    Ok(books)
}

#[orca_struct(args)]
pub struct WantArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Book ids to mark Wanted. Comma-separated on the CLI.
    #[arg(long, value_delimiter = ',', required = true)]
    pub book_ids: Vec<String>,
    /// Which format to mark Wanted.
    #[arg(long, value_enum, default_value_t = Format::Ebook)]
    #[serde(default)]
    pub format: Format,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct WantResult {
    pub id: String,
    pub format: Format,
    pub wanted: bool,
    pub error: Option<String>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct WantReport {
    pub results: Vec<WantResult>,
}

pub async fn want_books(ll: &Ll, args: &WantArgs) -> Result<WantReport> {
    if args.book_ids.is_empty() {
        bail!("pass at least one book id");
    }
    for id in &args.book_ids {
        check_id(id)?;
    }
    let mut report = WantReport {
        results: Vec::new(),
    };
    for id in &args.book_ids {
        for format in args.format.singles() {
            let result = ll.queue_book(id, format.ll_types()[0]).await;
            report.results.push(WantResult {
                id: id.clone(),
                format,
                wanted: result.is_ok(),
                error: result.err().map(|e| format!("{e:#}")),
            });
        }
    }
    Ok(report)
}

/// Mark books Wanted for the ebook, audiobook or both (`queueBook`).
#[orca_tool(
    domain = "lazylibrarian",
    verb = "books.want",
    role = "admin",
    execute_gated = true
)]
async fn lazylibrarian_books_want(args: WantArgs, _ctx: &ToolCtx) -> Result<WantReport> {
    want_books(&connect(&args.name).await?, &args).await
}

#[orca_struct(args)]
pub struct SearchArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Which format's wanted books to search for.
    #[arg(long, value_enum, default_value_t = Format::Both)]
    #[serde(default = "both")]
    pub format: Format,
}

fn both() -> Format {
    Format::Both
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct StartedReport {
    pub format: Format,
    pub started: bool,
}

pub async fn search_wanted(ll: &Ll, format: Format) -> Result<StartedReport> {
    let ty = match format {
        Format::Both => None,
        f => f.ll_types().first().copied(),
    };
    ll.force_search(ty).await?;
    Ok(StartedReport {
        format,
        started: true,
    })
}

/// Search providers for every Wanted book (`forceBookSearch`). Runs in the
/// background on the server.
#[orca_tool(
    domain = "lazylibrarian",
    verb = "wanted.search",
    role = "admin",
    execute_gated = true
)]
async fn lazylibrarian_wanted_search(args: SearchArgs, _ctx: &ToolCtx) -> Result<StartedReport> {
    search_wanted(&connect(&args.name).await?, args.format).await
}

#[orca_struct(args)]
pub struct ScanArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Which library to rescan.
    #[arg(long, value_enum, default_value_t = Format::Both)]
    #[serde(default = "both")]
    pub format: Format,
    /// Rescan only this author's books.
    #[arg(long)]
    pub author_id: Option<String>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct ScanReport {
    pub started: Vec<Format>,
    pub failed: Vec<Failure>,
}

pub async fn scan_library(ll: &Ll, args: &ScanArgs) -> Result<ScanReport> {
    let mut report = ScanReport {
        started: Vec::new(),
        failed: Vec::new(),
    };
    for format in args.format.singles() {
        match ll
            .scan(format.ll_types()[0], args.author_id.as_deref())
            .await
        {
            Ok(()) => report.started.push(format),
            Err(e) => report.failed.push(Failure {
                id: format!("{format:?}").to_lowercase(),
                error: format!("{e:#}"),
            }),
        }
    }
    if report.started.is_empty() {
        bail!(
            "no scan started: {}",
            report
                .failed
                .iter()
                .map(|f| f.error.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(report)
}

/// Rescan the ebook and/or audiobook library (`forceLibraryScan`,
/// `forceAudioBookScan`). Runs in the background on the server.
#[orca_tool(
    domain = "lazylibrarian",
    verb = "library.scan",
    role = "admin",
    execute_gated = true
)]
async fn lazylibrarian_library_scan(args: ScanArgs, _ctx: &ToolCtx) -> Result<ScanReport> {
    scan_library(&connect(&args.name).await?, &args).await
}

#[orca_struct(args)]
pub struct SkipArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Book ids to mark Skipped. Comma-separated on the CLI.
    #[arg(long, value_delimiter = ',', required = true)]
    pub book_ids: Vec<String>,
    /// Which format to mark Skipped.
    #[arg(long, value_enum, default_value_t = Format::Ebook)]
    #[serde(default)]
    pub format: Format,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct SkipResult {
    pub id: String,
    pub format: Format,
    pub skipped: bool,
    pub error: Option<String>,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct SkipReport {
    pub results: Vec<SkipResult>,
}

pub async fn skip_books(ll: &Ll, args: &SkipArgs) -> Result<SkipReport> {
    if args.book_ids.is_empty() {
        bail!("pass at least one book id");
    }
    for id in &args.book_ids {
        check_id(id)?;
    }
    let mut report = SkipReport {
        results: Vec::new(),
    };
    for id in &args.book_ids {
        for format in args.format.singles() {
            let result = ll.unqueue_book(id, format.ll_types()[0]).await;
            report.results.push(SkipResult {
                id: id.clone(),
                format,
                skipped: result.is_ok(),
                error: result.err().map(|e| format!("{e:#}")),
            });
        }
    }
    Ok(report)
}

/// Mark books Skipped for the ebook, audiobook or both (`unqueueBook`), so
/// searches stop grabbing a book whose releases keep failing. LazyLibrarian's
/// API has no per-release blocklist; this is its skip.
#[orca_tool(
    domain = "lazylibrarian",
    verb = "books.skip",
    role = "admin",
    execute_gated = true
)]
async fn lazylibrarian_books_skip(args: SkipArgs, _ctx: &ToolCtx) -> Result<SkipReport> {
    skip_books(&connect(&args.name).await?, &args).await
}

#[orca_struct(args)]
pub struct ProcessArgs {
    /// Registered lazylibrarian endpoint name.
    #[arg(long)]
    pub name: String,
    /// Directory on the LazyLibrarian host to import from. Omitted, its
    /// configured download directories.
    #[arg(long)]
    pub dir: Option<String>,
    /// Import without checking the download client's state.
    #[arg(long)]
    #[serde(default)]
    pub ignore_client: bool,
}

#[orca_struct]
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessReport {
    pub dir: Option<String>,
    pub processed: bool,
}

pub async fn process_downloads(ll: &Ll, args: &ProcessArgs) -> Result<ProcessReport> {
    ll.force_process(args.dir.as_deref(), args.ignore_client)
        .await?;
    Ok(ProcessReport {
        dir: args.dir.clone(),
        processed: true,
    })
}

/// Import completed downloads into the library (`forceProcess`). Runs to
/// completion before returning.
#[orca_tool(
    domain = "lazylibrarian",
    verb = "downloads.process",
    role = "admin",
    execute_gated = true
)]
async fn lazylibrarian_downloads_process(
    args: ProcessArgs,
    _ctx: &ToolCtx,
) -> Result<ProcessReport> {
    process_downloads(&connect(&args.name).await?, &args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_routes_give_a_clear_error() {
        let e = require_routes::<String>("ll", &[]).unwrap_err().to_string();
        assert_eq!(
            e,
            "endpoint ll has no routes; set routes (base_url is no longer used)"
        );
        assert!(require_routes("ll", &["http://ll.test".to_string()]).is_ok());
    }

    fn book(id: &str, ebook: Option<&str>, audio: Option<&str>) -> Book {
        Book {
            id: id.to_string(),
            title: id.to_string(),
            subtitle: None,
            author_id: Some("A1".to_string()),
            author: None,
            isbn: None,
            language: None,
            published: None,
            ebook_status: ebook.map(str::to_string),
            audio_status: audio.map(str::to_string),
        }
    }

    fn books_args(status: Option<&str>, audio: Option<&str>, limit: Option<u32>) -> BooksArgs {
        BooksArgs {
            name: "ll".to_string(),
            status: status.map(str::to_string),
            audio_status: audio.map(str::to_string),
            author_id: Some("A1".to_string()),
            limit,
        }
    }

    #[test]
    fn author_books_filter_by_status_then_limit() {
        let all = vec![
            book("B1", Some("Skipped"), Some("Wanted")),
            book("B2", Some("Open"), None),
            book("B3", Some("open"), Some("Wanted")),
            book("B4", Some("Open"), Some("Open")),
        ];
        let ids = |a: &BooksArgs| -> Vec<String> {
            filter_author_books(all.clone(), a)
                .into_iter()
                .map(|b| b.id)
                .collect()
        };
        assert_eq!(ids(&books_args(None, None, None)).len(), 4);
        assert_eq!(
            ids(&books_args(Some("Open"), None, None)),
            ["B2", "B3", "B4"]
        );
        assert_eq!(ids(&books_args(Some("Open"), None, Some(2))), ["B2", "B3"]);
        assert_eq!(ids(&books_args(None, Some("Wanted"), None)), ["B1", "B3"]);
        assert_eq!(ids(&books_args(Some("Open"), Some("Wanted"), None)), ["B3"]);
        assert_eq!(ids(&books_args(None, None, Some(1))), ["B1"]);
    }

    #[test]
    fn formats_map_to_upstream_types() {
        assert_eq!(Format::Ebook.ll_types(), &["eBook"]);
        assert_eq!(Format::Both.ll_types(), &["eBook", "AudioBook"]);
    }

    #[test]
    fn tools_declare_roles_and_gating() {
        let role = |n| plugin_toolkit::dispatch::required_role(n);
        for n in [
            "lazylibrarian.authors.list",
            "lazylibrarian.books.list",
            "lazylibrarian.parity.list",
        ] {
            assert_eq!(role(n), Some("read"), "{n}");
        }
        let gated = plugin_toolkit::dispatch::execute_gated_names();
        for n in [
            "lazylibrarian.books.want",
            "lazylibrarian.wanted.search",
            "lazylibrarian.library.scan",
            "lazylibrarian.books.skip",
            "lazylibrarian.downloads.process",
        ] {
            assert_eq!(role(n), Some("admin"), "{n}");
            assert!(gated.contains(&n), "{n}");
        }
    }
}
