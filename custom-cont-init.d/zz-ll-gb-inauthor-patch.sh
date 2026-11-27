#!/usr/bin/with-contenv bash
# shellcheck shell=bash
set -uo pipefail

GB="/app/lazylibrarian/lazylibrarian/gb.py"
log() { echo "[ll-gb-inauthor-patch] $*"; }

[[ -f "${GB}" ]] || { log "gb.py missing, skipping"; exit 0; }

python3 - "$GB" <<'PYEOF' || { log "patch error, leaving source unpatched"; exit 0; }
import io, sys, py_compile
path = sys.argv[1]
old = "quote(f'inauthor:\"{unaccented(authorname, only_ascii=False)}\"')"
new = "quote(f'inauthor:{unaccented(authorname, only_ascii=False)}')"
s = io.open(path, encoding="utf-8").read()
if new in s and old not in s:
    print("gb:already")
    sys.exit(0)
if old not in s:
    print("gb FAIL: anchor not found")
    sys.exit(1)
io.open(path, "w", encoding="utf-8").write(s.replace(old, new))
py_compile.compile(path, doraise=True)
print("gb:patched")
PYEOF

log "google books inauthor patch applied"
