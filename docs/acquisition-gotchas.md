# LazyLibrarian acquisition gotchas (operational KB)

Field notes from a large book/audiobook acquisition run. These are matcher,
status-reading, and import traps that make LazyLibrarian *look* broken when it is
not — plus the settings that fix them. Complements the setup notes in
[lazylibrarian.md](lazylibrarian.md).

## 1. Edition fragmentation — verify status across ALL editions of a title

When LazyLibrarian searches a title, its metadata handler adds newly matched
editions as fresh `Skipped` rows. The `Wanted`/`Snatched` state frequently lands
on a **different edition row** (a metadata-provider edition ID, or a series row)
than the canonical `bookid` you are watching. Reading the canonical row alone
shows "still Skipped" and looks like the queue failed — when the book is actually
queued and snatching fine on a sibling row.

**Rule:** to verify queue state, group every edition row by *title + author* and
take the best status across the group. Never trust a single `bookid`, and never
trust the queue-time API response — read the database fresh at the end. A common
provider (e.g. Libgen) usually is not the blocker; a "Skipped" reading here is
typically a fragmentation artifact, not "no release found."

## 2. Fuzzy matcher grabs the wrong book for generic/substring titles

Where a volume's title is a substring or duplicate of its series name, the fuzzy
matcher mis-binds. Example: a book literally titled *"The Dark Tower"* (series
book 7) repeatedly matched and imported the *"The Waste Lands"* (book 3) release,
so the library folder wore book 7's name but held book 3's audio. Re-queueing
just re-snatched the wrong release.

**Fix pattern:** null the bogus file pointer on the row; find the *specific*
correct release by hand in Prowlarr/your indexer; grab that exact release
directly into the download client (bypassing the matcher); bind the snatch record
to that download ID so the mapping cannot drift; then force post-processing. Split
any co-mingled files into per-book folders by moving (never deleting — it may be
the only copy).

## 3. `match_ratio` default of 80 silently rejects usenet audiobook releases

LazyLibrarian is often not failing to *find* audiobooks — it is *discarding* them.
Usenet release names embed series/book-number prefixes and collection names and
drop leading articles (e.g. `Author - (Series II) The Title`, `[Series #8]`,
`... - The Collection`). This drags the fuzzy title-match score to **78–79.9%**,
just under the default `match_ratio = 80`, so the result is logged as "Nearest
match" and rejected. Observed in one run: ~899 rejected vs ~124 accepted across
audio searches, near-misses clustered at 78–79.9%; 80.08% accepted while 79.93%
was rejected. This is why raw indexer→download-client grabs (no fuzzy name-gate)
succeed while the same release never lands via LazyLibrarian.

**Fix:** set `match_ratio = 75` under `[General]` (unset defaults to 80). Because
LazyLibrarian rewrites `config.ini` on shutdown, edit it while the container is
**stopped**, or change it through the web UI. Trade-off: 75 is more permissive, so
pair it with a bounded blocklist (see #6) for wrong grabs.

Note: torrent audiobook providers tend to have thin real-audio coverage (dead
0-seed torrents, ebooks mislabeled as audio); usenet is usually where the real
audio is, so the match gate — not provider coverage — is the true blocker.

## 4. Anthologies contaminate `seriesauthors`

LazyLibrarian derives a series→author link from *every contributor of every member
book* of a series. An anthology (many authors) therefore pollutes an otherwise
single-author series with extra authors. A series exists only as a function of its
member books, and its authorship is *derived* — so you cannot durably "pin" an
author onto a member-less series; the link regenerates from members. Keep
anthologies out of single-author series, or accept the derived multi-author link.

## 5. Metadata provider returns junk for niche / apostrophe titles

The Google Books metadata path returns garbage for less-famous or possessive
titles — foreign-language editions, wrong books, short-story fragments. **Never
use a `rows[0]` fallback** in acquisition scripts: it queues junk that then
downloads the wrong or foreign edition. Require a strict match: author surname
present in the author field **AND** title-substring match **AND** an all-ASCII
title. Trying multiple phrasings helps ("Title Author", "Author Title", "Title"),
and stripping apostrophes improves hit rate. Famous authors match fine; niche and
possessive titles do not.

## 6. `blocklist_timer` — keep the blocklist healing-safe

The default `blocklist_timer` (~1h) is short enough that dead 0-seed grabs
re-storm the queue. Set a bounded value (e.g. `172800` seconds / 48h) so bad
releases back off **but auto-expire** — a blocklisted/Skipped item must remain
re-Wantable so acquisition can self-heal. Never configure a permanent lockout.

## 7. Importer bridge race — post-processed files vanish before an external feed sees them

LazyLibrarian post-processes completed files into an ingest directory
(`.../books/_ingest/<Author>/<Title>/`) and cleans them shortly after. An external
importer that watches only the *downloads* directory never sees them, so the book
downloads fine but never imports and the DB points at now-gone files. Observed
losses of several titles this way.

**Fix:** have the durable importer sweep **both** the downloads directory **and**
the post-process ingest directory, or disable move-on-import so completed files
stay in downloads until the importer has bridged them. Then re-Want anything lost
to the race.
