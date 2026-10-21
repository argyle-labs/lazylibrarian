<p align="center">
  <img src="assets/icon-256.png" width="120" alt="lazylibrarian" />
</p>

# lazylibrarian

LazyLibrarian finds, imports, and organizes ebooks and audiobooks.

A first-party [orca](https://github.com/argyle-labs/orca) plugin (service-backend).

This repo is **self-contained** — the steps below run lazylibrarian **by hand, without orca**. orca automates exactly this (same image, ports, and data) through one generic surface.

---

## Run it without orca

### Docker Compose

```yaml
# compose.yml
services:
  lazylibrarian:
    image: lscr.io/linuxserver/lazylibrarian:latest
    container_name: lazylibrarian
    restart: unless-stopped
    ports:
      - "5299:5299/tcp"   # web UI
    volumes:
      - ./config:/config
      - /path/to/books:/books
      - /path/to/downloads:/downloads
```

```sh
docker compose up -d
```

### Other runtimes

**Podman** — the compose above works with `podman compose up -d`, or run it directly:

```sh
podman run -d --name lazylibrarian --restart unless-stopped \
    -p 5299:5299/tcp \
    -v ./config:/config \
    -v /path/to/books:/books \
    -v /path/to/downloads:/downloads \
    lscr.io/linuxserver/lazylibrarian:latest
```

**LXC** — on a container-capable LXC (e.g. a Proxmox LXC with nesting enabled) run the same image via Docker/Podman as above, or install lazylibrarian from upstream directly on the guest: <https://gitlab.com/LazyLibrarian/LazyLibrarian>.

**VM** — install lazylibrarian from upstream (<https://gitlab.com/LazyLibrarian/LazyLibrarian>) or run the same container image inside the VM; expose port `5299`.

**Unraid** — add via *Community Applications*, or *Docker → Add Container* with image `lscr.io/linuxserver/lazylibrarian:latest`, port `5299`, and the volume paths above.

### Ports & data

| | |
|---|---|
| Default port | `5299` |
| Upstream | <https://gitlab.com/LazyLibrarian/LazyLibrarian> |
| Operator notes | [lazylibrarian.md](docs/lazylibrarian.md) |


### Backup & restore

Back up the config/data volume(s) above — that's the whole service state (stop the container first for a clean copy). Restore by putting them back and starting it.

> With orca this is **`service.backup` / `service.restore`** — location-agnostic (docker / podman / lxc / vm), one command regardless of where lazylibrarian runs. No per-service backup script.

## With orca

orca drives the service lifecycle through the generic `service.*` surface:

```sh
orca service.deploy lazylibrarian      # render + launch on any supported runtime
orca service.status lazylibrarian      # health + rich diagnostics (typed payload)
orca service.backup lazylibrarian      # location-agnostic backup (tar; PBS on Proxmox)
orca service.configure lazylibrarian   # apply config via the upstream API
```

Library tools translate the LazyLibrarian API (`/api?cmd=...`). The endpoint stores
routes only; the API key is the scoped secret `api_key`. Reads need `read`, writes
need `admin`; writes run when called and accept `dryRun` for a preview.

| Tool | Upstream command |
|---|---|
| `lazylibrarian.authors.list` | `getIndex` |
| `lazylibrarian.books.list` | `getAllBooks` (`status`, `audiostatus`) |
| `lazylibrarian.parity.list` | `getAllBooks`, books whose ebook and audiobook availability differ |
| `lazylibrarian.books.want` | `queueBook` (`type=eBook`/`AudioBook`) |
| `lazylibrarian.wanted.search` | `forceBookSearch` |
| `lazylibrarian.library.scan` | `forceLibraryScan`, `forceAudioBookScan` |
| `lazylibrarian.books.skip` | `unqueueBook` (`type=eBook`/`AudioBook`), marks Skipped |
| `lazylibrarian.downloads.process` | `forceProcess` (`dir`, `ignoreclient`) |

`books.list` takes `limit`. `parity.list` and the media `units` read the whole
`getAllBooks` table: upstream offers `limit` without an offset, so a partial read
would silently drop books from the report.

**Upgrading from an older endpoint:** `base_url` and the inline `api_key` are gone.
Set the endpoint's routes and store the key as its `api_key` secret; an endpoint
without routes fails with `endpoint <name> has no routes`.

The media facet registers as the ebook and audiobook acquirer (`list`, `search`,
`library_add`, `units`).

## Layout

- `src/` — the plugin (pure Rust): the `ServiceBackend` descriptor, API client, library tools, media facet.
- `docs/` — standalone operator notes.
- [CAPABILITIES.md](CAPABILITIES.md) — the service-backend contract checklist.
- `assets/` — plugin icon.
