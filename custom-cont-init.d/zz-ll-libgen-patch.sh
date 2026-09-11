#!/usr/bin/with-contenv bash
# shellcheck shell=bash
# Re-apply the LazyLibrarian libgen (Library Genesis) direct-provider fixes on
# every container start, AFTER 99-ll-source-update.sh has pinned the app source.
#
# Why this exists: the pinned upstream LazyLibrarian (commit 02af0464) cannot
# actually download from the current libgen.la/.li "tablelibgen" (edition.php)
# layout. Four bugs, fixed here against directparser.py + downloadmethods.py:
#   1. Title read from td[0].text = series junk -> pull it from the edition.php
#      anchor so results survive the fuzzy title match.
#   2. Over-specified search terms (subtitle + leading series index) return 0
#      rows -> strip bookSub and a leading "N.N " series index.
#   3. Anti-hotlink: ads.php/get.php return empty without a same-host Referer ->
#      send one on both the ads fetch and the final file download.
#   4. Download-link picker took the first anchor and required http:// -> scan
#      all anchors for the relative get.php?...&key=... link and resolve it.
#
# Runs as root via linuxserver's /custom-cont-init.d, in ascending filename
# order. NOTE: the collation ignores the hyphen, so a "99b-" name would sort
# BEFORE "99-ll-source-update.sh" (b < l) and get its patches wiped by the
# source pin. A letter prefix ("zz-") sorts after any "99-" script, so this
# always runs AFTER the source is pinned. Idempotent and fail-safe: on any
# error it leaves the source as-is and never blocks container start.
set -uo pipefail

APP="/app/lazylibrarian/lazylibrarian"
log() { echo "[ll-libgen-patch] $*"; }

[[ -f "${APP}/directparser.py" ]] || { log "directparser.py missing, skipping"; exit 0; }

python3 - "$APP" <<'PYEOF' || { log "patch error — leaving source unpatched"; exit 0; }
import io, sys, py_compile
APP = sys.argv[1]
DP = f"{APP}/directparser.py"
DM = f"{APP}/downloadmethods.py"
changed = []

def edit(path, pairs, guard):
    s = io.open(path, encoding="utf-8").read()
    if guard in s:
        return "already"
    for old, new in pairs:
        if old not in s:
            raise RuntimeError(f"anchor not found in {path}: {old[:60]!r}")
        s = s.replace(old, new, 1)
    io.open(path, "w", encoding="utf-8").write(s)
    py_compile.compile(path, doraise=True)
    return "patched"

# ---- directparser.py ----
dp_pairs = [
  # (1) title from edition.php anchor
  ("                            title = td[0].text.split('\\n')[0].strip()\n",
   "                            title = ''\n"
   "                            _t0 = BeautifulSoup(str(td[0]), 'html5lib')\n"
   "                            for _a in _t0.find_all('a'):\n"
   "                                if 'edition.php' in (_a.get('href') or ''):\n"
   "                                    _txt = _a.text.strip()\n"
   "                                    if _txt and not _txt.replace('-', '').replace(' ', '').isdigit():\n"
   "                                        title = _txt\n"
   "                                        break\n"
   "                            if not title:\n"
   "                                title = td[0].text.split('\\n')[0].strip()\n"),
  # (2) clean search term: strip leading series index + bookSub
  ("    sterm = make_unicode(book['searchterm'])\n",
   "    sterm = make_unicode(book['searchterm'])\n"
   "    _p = sterm.split(' ', 1)\n"
   "    if len(_p) > 1 and _p[0].replace('.', '', 1).isdigit():\n"
   "        sterm = _p[1]\n"
   "    _sub = make_unicode(book.get('bookSub', '') or '')\n"
   "    if _sub:\n"
   "        _subn = ' '.join(''.join(c if (c.isalnum() or c.isspace()) else ' ' for c in _sub).split()).lower()\n"
   "        _stn = ' '.join(sterm.split())\n"
   "        if _subn and _subn in _stn.lower():\n"
   "            sterm = _stn[:_stn.lower().find(_subn)].strip()\n"
   "    sterm = sterm.split(':')[0].strip()\n"
   "    if sterm and sterm != book['searchterm']:\n"
   "        book = dict(book)\n"
   "        book['searchterm'] = sterm\n"),
  # (3) referer on the ads redirect fetch (host derived from the url)
  ("                            bookresult, success = fetch_url(url)\n",
   "                            bookresult, success = fetch_url(url, headers={'User-Agent': get_user_agent(), "
   "'Referer': ('{}://{}/'.format(urlparse(url).scheme, urlparse(url).netloc) if url.startswith('http') else host + '/')})\n"),
  # (4) scan all anchors for the real get.php download link (relative or absolute)
  ("""                                for link in new_soup.find_all('a'):
                                    output = link.get('href')
                                    if output:
                                        if ('/get.php' in output or '/download/' in output
                                                or '/book/' in output or '/fiction/' in output
                                                or '/main/' in output) and output.startswith('http'):
                                            url = output
                                            break
                                        nhost = urlparse(url)
                                        nurl = urlparse(output)
                                        # noinspection PyProtectedMember
                                        nurl = nurl._replace(scheme=nhost.scheme)
                                        # noinspection PyProtectedMember
                                        nurl = nurl._replace(netloc=nhost.netloc)
                                        url = nurl.geturl()
                                        break
""",
   """                                _adsurl = url
                                _dl = None
                                for link in new_soup.find_all('a'):
                                    output = link.get('href') or ''
                                    if ('get.php' in output or '/download/' in output
                                            or '/main/' in output or '/fiction/' in output):
                                        _dl = output
                                        break
                                if _dl:
                                    if _dl.startswith('http'):
                                        url = _dl
                                    else:
                                        _b = urlparse(_adsurl)
                                        url = '{}://{}/{}'.format(_b.scheme, _b.netloc, _dl.lstrip('/'))
"""),
  # import get_user_agent (used by the referer fix)
  ("from lazylibrarian.cache import fetch_url\n",
   "from lazylibrarian.cache import fetch_url\nfrom lazylibrarian.common import get_user_agent\n"),
]
try:
    r = edit(DP, dp_pairs, guard="'Referer': ('{}://{}/'.format(urlparse(url)")
    changed.append(f"directparser:{r}")
except Exception as e:
    print(f"directparser FAIL: {e}"); sys.exit(1)

# ---- downloadmethods.py: referer on the final file download ----
dm_old = "    headers = {'Accept-encoding': 'gzip', 'User-Agent': get_user_agent()}\n"
dm_new = (dm_old +
    "    try:\n"
    "        _rp = urlsplit(make_unicode(dl_url))\n"
    "        headers['Referer'] = f\"{_rp.scheme}://{_rp.netloc}/\"\n"
    "    except Exception:\n"
    "        pass\n")
try:
    s = io.open(DM, encoding="utf-8").read()
    if 'headers[\'Referer\'] = f"{_rp.scheme}' in s:
        changed.append("downloadmethods:already")
    elif dm_old not in s:
        raise RuntimeError("downloadmethods header anchor not found")
    else:
        io.open(DM, "w", encoding="utf-8").write(s.replace(dm_old, dm_new))
        py_compile.compile(DM, doraise=True)
        changed.append("downloadmethods:patched")
except Exception as e:
    print(f"downloadmethods FAIL: {e}"); sys.exit(1)

print(" ".join(changed))
PYEOF

log "libgen patches applied"
