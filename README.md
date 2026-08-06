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

orca drives this plugin through the single generic `service.*` surface — no per-plugin tools:

```sh
orca service.deploy lazylibrarian      # render + launch on any supported runtime
orca service.status lazylibrarian      # health + rich diagnostics (typed payload)
orca service.backup lazylibrarian      # location-agnostic backup (tar; PBS on Proxmox)
orca service.configure lazylibrarian   # apply config via the upstream API
```

## Layout

- `src/` — the plugin (pure Rust): the `ServiceBackend` descriptor + `configure` / `status`.
- `docs/` — standalone operator notes.
- [CAPABILITIES.md](CAPABILITIES.md) — the service-backend contract checklist.
- `assets/` — plugin icon.
