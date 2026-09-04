#!/usr/bin/with-contenv bash
# shellcheck shell=bash
# Pin LazyLibrarian's application source to a known-good upstream commit.
#
# Why this exists: the linuxserver image bundles a LazyLibrarian source
# checkout that lags upstream by many commits, and its in-app updater is a
# no-op for the "source DOCKER" install type. That stale code cannot talk to
# HardCover's current GraphQL schema (author-id resolution fails) and carries a
# JSONCache write bug. This hook re-applies the pinned upstream source over
# /app/lazylibrarian on every container start so the fix survives recreates.
#
# Runs as root via linuxserver's /custom-cont-init.d before services start.
# Fails loud but never blocks container start: on any error the image's bundled
# (older) code is left in place and LazyLibrarian still comes up.
set -uo pipefail

PIN_SHA="02af04640b3dd91c4b319e893a8ac15f71d74f34"
APP_DIR="/app/lazylibrarian"
MARKER="${APP_DIR}/.pinned_source_sha"
ARCHIVE="https://gitlab.com/LazyLibrarian/LazyLibrarian/-/archive/${PIN_SHA}/LazyLibrarian-${PIN_SHA}.tar.gz"
WORK="/tmp/ll-source-update"

log() { echo "[ll-source-update] $*"; }

# Idempotent: if this exact commit is already in place, do nothing.
if [[ -f "${MARKER}" ]] && [[ "$(cat "${MARKER}" 2>/dev/null)" == "${PIN_SHA}" ]]; then
    log "already pinned to ${PIN_SHA}, skipping"
    exit 0
fi

log "pinning LazyLibrarian source to ${PIN_SHA}"
rm -rf "${WORK}"
mkdir -p "${WORK}"

if ! curl -fsSL --max-time 120 -o "${WORK}/src.tar.gz" "${ARCHIVE}"; then
    log "ERROR: download failed from ${ARCHIVE} — leaving bundled source in place"
    exit 0
fi

if ! tar xzf "${WORK}/src.tar.gz" -C "${WORK}"; then
    log "ERROR: extract failed — leaving bundled source in place"
    exit 0
fi

SRC="${WORK}/LazyLibrarian-${PIN_SHA}"
if [[ ! -f "${SRC}/LazyLibrarian.py" ]]; then
    log "ERROR: extracted tree missing LazyLibrarian.py — leaving bundled source in place"
    exit 0
fi

# Swap: back up the bundled tree once, then replace.
if [[ -d "${APP_DIR}" ]] && [[ ! -d "${APP_DIR}.image" ]]; then
    cp -a "${APP_DIR}" "${APP_DIR}.image" || true
fi
rm -rf "${APP_DIR}"
cp -a "${SRC}" "${APP_DIR}"
echo "${PIN_SHA}" > "${MARKER}"
rm -rf "${WORK}"
log "pinned ${APP_DIR} to ${PIN_SHA}"
