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

## Getting it actually working (operational notes)

LazyLibrarian is powerful but has several non-obvious setup requirements. A fresh
install will appear "empty" and download nothing until these are addressed.

### 1. Metadata provider (`book_api`)

`book_api` selects a **single** active metadata provider — LazyLibrarian has no
runtime fallback between providers, so pick the best working one and keep the
others' keys configured to switch.

- **GoodReads** — dead. The Goodreads API was retired in 2020 and issues no keys;
  a fresh install may default to it and then silently identify nothing.
- **HardCover** — series-aware with proper author identity; the best choice for a
  series-organized library. Requires a free account token (see below).
- **GoogleBooks** — works as a fallback, but returns **no series data** and has
  weak author disambiguation (an `inauthor:` search pulls books by other authors
  of the same name). Keyless requests are rate-limited (HTTP 429); set a Google
  Books API key in `GB_API` and `GB_COUNTRY` (e.g. `US`) to clear it.
- **OpenLibrary** — keyless, but openlibrary.org may be unreachable from some
  networks (the Internet Archive tarpits egress IPs under load); verify
  connectivity before relying on it.

Set the provider under **Settings → Interface → Book/Author API**.

**HardCover token — the non-obvious part.** The token is stored per-user in the
`users` table (`hc_token`, and `hc_id` = your numeric HardCover id). LazyLibrarian
sends `hc_token` **verbatim** as the `authorization` header, so it MUST include
the `Bearer ` prefix (`Bearer eyJ…`). A raw JWT yields *"Malformed Authorization
header"*, every HardCover query returns empty, and LazyLibrarian falls back to
synthetic `LL<hash>` author ids and imports zero books ("Rejecting authorid …,
no authorname"). Set the token through the web UI (which adds the prefix), or if
writing the DB directly, store the full `Bearer …` string. Also set `BOOK_API =
HardCover` and `HC_API = 1`. Note HardCover's API is beta and may change; tokens
expire (~1 year).

### 2. Populate before you scan

A library scan **matches files against books already known to the database** — it
does **not** discover authors from disk. On an empty database a scan imports
nothing. The correct order is:

1. **Add authors** (Author search → add). This seeds each author's full
   bibliography and series from the metadata provider.
2. **Run a library scan** (`forceAudioBookScan` for audiobooks,
   `forceLibraryScan` for ebooks). Books found on disk are marked **Open**
   (owned); everything else stays **Skipped**.
3. **Missing entries in a series = your want-list.** Mark the ones you want as
   **Wanted** to have them searched.

Newly imported books/authors default to **Skipped** (`NEWBOOK_STATUS`,
`NEWAUDIO_STATUS`, `NEWAUTHOR_STATUS`), and `IMP_AUTOSEARCH` is off — so importing
an author's whole catalogue is safe and will not trigger downloads until you
explicitly mark titles Wanted.

### 3. Config-persistence trap

LazyLibrarian rewrites `config.ini` from its in-memory state on shutdown. Editing
`config.ini` while the app is running is silently overwritten on the next
restart. **Change settings through the web UI**, or stop the container before
editing the file by hand.

### 4. One library dir per media type

`audio_dir` / `ebook_dir` each point at a single directory tree. If part of your
collection lives outside that tree (e.g. an Audible/Libation output folder kept
separate), LazyLibrarian will not see those titles as owned and may try to
re-acquire them. Keep owned content under the scanned directory, or symlink the
external folder into it, so "owned vs missing" is computed correctly.

### 5. Folder template with series subfolders

For a clean `Author/Series/NN - Title` layout that also handles standalone books,
set **Settings → Processing → Audiobook destination folder** to:

```
$Author/{$Series/}{$PadNum - }$Title
```

The `{ }` groups collapse to empty when a variable is blank, so series books get
a series subfolder with a zero-padded number and standalones fall back cleanly to
`Author/Title`. An equivalent template applies to the ebook destination.

### 6. Indexers via Prowlarr

Rather than adding Torznab/Newznab feeds by hand, register LazyLibrarian in
**Prowlarr → Settings → Apps** (implementation *LazyLibrarian*, full sync) so it
inherits every indexer automatically — the same model as the *arr* apps. Include
the book/audiobook categories: **Books (7000)**, **Books/EBook (7020)**, and
**Audio/Audiobook (3030)**. Attach at least one usenet (SABnzbd) and/or torrent
(qBittorrent) download client under **Settings → Downloaders**.

## Application source pinning

The linuxserver image bundles a LazyLibrarian source checkout that lags upstream
by many commits, and its in-app updater is a **no-op** for the `source DOCKER`
install type (the tarball-download branch requires `install_type == 'source'`
exactly, so `cmd=update` only bumps the reported version). That stale code cannot
talk to HardCover's current GraphQL schema and carries a JSONCache write bug.

To fix this durably, [`custom-cont-init.d/99-ll-source-update.sh`](../custom-cont-init.d/99-ll-source-update.sh)
downloads a pinned upstream commit and swaps it over `/app/lazylibrarian` on
every container start (linuxserver runs executable scripts in `/custom-cont-init.d`
as root before services start). The compose file mounts this directory. The
script is idempotent (a marker file skips re-work), fails loud, and never blocks
startup — on any download/extract error the bundled code is left in place. Bump
`PIN_SHA` in the script to move to a newer upstream commit.

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
