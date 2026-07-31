//! Dynamic (subprocess) entrypoint for the lazylibrarian plugin.
//!
//! The toolkit's `serve_service_plugin!` emits `fn main`, serving this plugin over the orca
//! socket. The plugin is a
//! `[[bin]]`, owns no runtime, and reaches orca only through the socket.
plugin_toolkit::serve_service_plugin! {
    name: "lazylibrarian",
    target_compat: "any",
    backend: lazylibrarian::LazylibrarianBackend::new("lazylibrarian"),
}
