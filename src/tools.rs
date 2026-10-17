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
    /// Only books with this ebook status (`Wanted`, `Open`, `Skipped`, ...).
    #[arg(long)]
    pub status: Option<String>,
    /// Only books with this audiobook status.
    #[arg(long)]
    pub audio_status: Option<String>,
    /// Only books by this author id.
    #[arg(long)]
    pub author_id: Option<String>,
    /// Return at most this many books (upstream `limit`, applied before the author filter).
    #[arg(long)]
    pub limit: Option<u32>,
}

pub async fn list_books(ll: &Ll, args: &BooksArgs) -> Result<Vec<Book>> {
    if let Some(id) = &args.author_id {
        check_id(id)?;
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
    let mut books = ll.books(&params).await?;
    if let Some(id) = &args.author_id {
        books.retain(|b| b.author_id.as_deref() == Some(id.as_str()));
    }
    Ok(books)
}

/// List books with ebook and audiobook status, ISBN, author, language and
/// publication date (`getAllBooks`).
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
    let books = connect(&args.name).await?.books(&[]).await?;
    Ok(parity::parity(books, args.include_ignored))
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
        ] {
            assert_eq!(role(n), Some("admin"), "{n}");
            assert!(gated.contains(&n), "{n}");
        }
    }
}
