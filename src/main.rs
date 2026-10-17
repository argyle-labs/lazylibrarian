plugin_toolkit::instrument::bootstrap!();

#[allow(unused_imports)]
use lazylibrarian::tools as _;

fn main() -> plugin_toolkit::anyhow::Result<()> {
    use plugin_toolkit::media::MediaType;
    plugin_toolkit::plugin::Plugin::named("lazylibrarian")
        .version(env!("CARGO_PKG_VERSION"))
        .tools(["lazylibrarian."])
        .service(lazylibrarian::LazylibrarianBackend::new("lazylibrarian"))
        .media(lazylibrarian::LlMedia::acquires(MediaType::Ebooks))
        .media(lazylibrarian::LlMedia::acquires(MediaType::Audiobooks))
        .serve()
}
