//! Dynamic (subprocess) entrypoint for the lazylibrarian plugin.
//!
//! LazyLibrarian is a **multi-facet** plugin: a deployable `service` AND a media
//! `downloaded_by` acquirer for ebooks + audiobooks. The unified
//! [`Plugin`](plugin_toolkit::plugin::Plugin) builder advertises all three facets
//! from one binary (one merged `backends` array, one composed dispatch), plus the
//! `lazylibrarian.*` endpoint-registry tools. Referencing the lib's types here
//! force-links its `#[orca_tool]` inventory so it survives linking.
plugin_toolkit::instrument::bootstrap!();

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
