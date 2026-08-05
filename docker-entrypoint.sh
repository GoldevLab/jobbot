#!/bin/sh
# Ensure the Fly volume is writable, then run JobBot as non-root.
set -eu

DATA_DIR="${RESUMA_DATA_DIR:-/data}"

if [ "$(id -u)" = "0" ]; then
  mkdir -p "$DATA_DIR" "$DATA_DIR/drafts" "$DATA_DIR/rate-limit"
  chown -R jobbot:jobbot "$DATA_DIR" 2>/dev/null || true
  exec gosu jobbot /app/jobbot "$@"
fi

mkdir -p "$DATA_DIR" "$DATA_DIR/drafts" "$DATA_DIR/rate-limit" 2>/dev/null || true
exec /app/jobbot "$@"
