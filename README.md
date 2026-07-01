# lazylibrarian

LazyLibrarian book (ebook/audiobook) downloader & manager — a first-party [orca](https://github.com/argyle-labs/orca) **service-backend**
plugin. It registers a `ServiceBackend` and exposes **no tools of its own**: orca
drives every plugin through the single generic `service.*` surface — `list`,
`deploy`, `backup`, `restore`, `configure`, `status`. Rich, lazylibrarian-specific data is
surfaced through the **typed `service.status` payload**, never bespoke tools (one
small API for the whole fleet).

**Runtimes:** docker,podman,lxc.

**Design — pure Rust, zero bash.** No `compose.yml`, `Dockerfile`, or provision
scripts. Deployment is rendered by orca's `deploy_target` from the backend's
`WorkloadSpec`; backup/restore run through the pluggable `BackupMethod` (tar for
containers/LXC, **Proxmox Backup Server** for Proxmox guests when available);
`configure`/`status` call the upstream API. The only per-plugin code is the
declarative descriptor plus `workload_spec`/`configure`/`status`.

See [CAPABILITIES.md](CAPABILITIES.md) for the contract checklist.

## Manual setup & management

The plugin automates lazylibrarian, but this repo is self-contained: the docs below (migrated + anonymized from a homelab runbook) let you deploy, configure, and operate it **entirely by hand** on any supported runtime.

- [lazylibrarian](docs/lazylibrarian.md)
