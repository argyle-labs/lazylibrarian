# LazyLibrarian

Ebook and audiobook automation. Monitors, downloads, and organizes books and audiobooks. Single instance handles both.

- **Host**: <host> (<ip>)
- **Port**: 5299
- **Image**: `lscr.io/linuxserver/lazylibrarian`
- **Compose**: [compose/lazylibrarian/docker-compose.yml](../../compose/lazylibrarian/docker-compose.yml)

## Volumes

| Host Path | Container Path | Description |
|-----------|---------------|-------------|
| `/opt/appdata/lazylibrarian` | `/config` | LazyLibrarian config and database |
| `/mnt/<host>/downloads` | `/downloads` | Downloads (NFS) |
| `/mnt/<host>/data/media/books` | `/data/media/books` | Ebook library (NFS) |
| `/mnt/<host>/data/media/audiobooks` | `/data/media/audiobooks` | Audiobook library (NFS) |

## Docker Mods

The compose includes two linuxserver mods:
- `universal-calibre` — enables Calibre for ebook conversion and metadata
- `lazylibrarian-ffmpeg` — enables FFmpeg for audiobook processing

## Download Clients

Configure in Settings → Downloaders:

| Client | Protocol | Host | Port |
|--------|----------|------|------|
| SABnzbd | NZB | <ip> | 8080 |
| qBittorrent | Torrent | <ip> | 8070 |

## Indexers

LazyLibrarian does not auto-sync from Prowlarr. Configure providers manually using Prowlarr's Newznab/Torznab proxy URLs.

**Get the Prowlarr API key**: Prowlarr UI → Settings → General → API Key

### Usenet (NZB)

Settings → Providers → Newznab:

| Name | URL | API Key | Notes |
|------|-----|---------|-------|
| NZBgeek (via Prowlarr) | `http://<ip>:9696/1/api` | *(Prowlarr API key)* | General Usenet — good books/audiobook coverage |

### Torrents

Settings → Providers → Torznab:

| Name | URL | API Key | Notes |
|------|-----|---------|-------|
| LimeTorrents (via Prowlarr) | `http://<ip>:9696/7/api` | *(Prowlarr API key)* | General torrents |
| The Pirate Bay (via Prowlarr) | `http://<ip>:9696/4/api` | *(Prowlarr API key)* | General torrents |
| comicat (via Prowlarr) | `http://<ip>:9696/6/api` | *(Prowlarr API key)* | Comics/manga |

> Prowlarr indexer IDs: NZBgeek=1, YTS=5, comicat=6, LimeTorrents=7, The Pirate Bay=4.
> If indexers are added/removed in Prowlarr, re-check IDs via `GET /api/v1/indexer`.

## Calibre Integration

LazyLibrarian can write directly into the Calibre library. Set the Calibre library path to `/data/media/books` in Settings → Processing. Calibre-Web will then serve the same library.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `TZ` | `Etc/UTC` | Timezone |
| `PUID` / `PGID` | `1000` / `100` | User/group ID (100 = Unraid users group) |
| `LAZYLIBRARIAN_IMAGE_TAG` | `latest` | Image tag |
| `LAZYLIBRARIAN_CONFIG_PATH` | `/opt/appdata/lazylibrarian` | Config directory |
| `LAZYLIBRARIAN_PORT` | `5299` | Host port |
| `DOWNLOADS_PATH` | `/mnt/<host>/downloads` | Downloads path |
| `MEDIA_PATH` | `/mnt/<host>/data/media` | Media library base path |

## Deploy

Deployed as a Portainer Git stack from `<github-org>/<repo>` main branch. Auto-updates every 5 minutes.

## Troubleshooting

```bash
docker logs lazylibrarian
mount | grep <host>  # verify NFS mounts
```
