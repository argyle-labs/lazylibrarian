# LazyLibrarian

Ebook and audiobook automation. Monitors, imports, and organizes books and audiobooks. A single instance handles both.

- **Port**: 5299 (web UI)
- **Image**: `lscr.io/linuxserver/lazylibrarian`
- **Upstream**: <https://gitlab.com/LazyLibrarian/LazyLibrarian>

## Volumes

| Container Path | Description |
|----------------|-------------|
| `/config` | LazyLibrarian config and database |
| `/downloads` | Staging area for incoming files |
| `/data/media/books` | Ebook library |
| `/data/media/audiobooks` | Audiobook library |

Map each container path to a host directory (or network share) that suits your setup.

## Docker Mods

The linuxserver image supports optional mods:

- `universal-calibre` — enables Calibre for ebook conversion and metadata.
- `lazylibrarian-ffmpeg` — enables FFmpeg for audiobook processing.

## Content providers

Configure your own content providers under Settings → Providers. LazyLibrarian
does not ship with any providers preconfigured — add the sources you are
entitled to use, following their own setup instructions.

## Calibre integration

LazyLibrarian can write directly into a Calibre library. Set the Calibre library
path to your books volume (e.g. `/data/media/books`) under Settings → Processing.
A Calibre-Web instance pointed at the same directory will then serve the library.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `TZ` | `Etc/UTC` | Timezone |
| `PUID` / `PGID` | `1000` / `1000` | User/group ID |
| `LAZYLIBRARIAN_IMAGE_TAG` | `latest` | Image tag |
| `LAZYLIBRARIAN_CONFIG_PATH` | — | Host path for `/config` |
| `LAZYLIBRARIAN_PORT` | `5299` | Host port |

## Troubleshooting

```bash
docker logs lazylibrarian
```
