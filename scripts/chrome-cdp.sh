#!/usr/bin/env bash
# Launch Chrome with remote debugging for JobBot CDP.
# Uses a dedicated profile by default so your main Chrome can stay open.
# First run: log into Google / LinkedIn in this window once.

set -euo pipefail
PROFILE="${JOBBOT_CHROME_PROFILE:-$HOME/.config/jobbot-chrome}"
PORT="${JOBBOT_CHROME_PORT:-9222}"
BIN="${JOBBOT_CHROME_BIN:-}"

mkdir -p "$PROFILE"

if [[ -z "$BIN" ]]; then
  for c in google-chrome-stable google-chrome chromium chromium-browser; do
    if command -v "$c" >/dev/null 2>&1; then
      BIN="$c"
      break
    fi
  done
fi

if [[ -z "$BIN" ]]; then
  echo "Chrome/Chromium not found" >&2
  exit 1
fi

# If something already answers on the port, reuse it.
if curl -fsS "http://127.0.0.1:${PORT}/json/version" >/dev/null 2>&1; then
  echo "Chrome CDP already up on port $PORT"
  curl -fsS "http://127.0.0.1:${PORT}/json/version" | head -c 300
  echo
  exit 0
fi

echo "Starting $BIN"
echo "  user-data-dir=$PROFILE"
echo "  remote-debugging-port=$PORT"
echo "Attach JobBot with JOBBOT_CHROME_CDP=http://127.0.0.1:$PORT"

exec "$BIN" \
  --remote-debugging-port="$PORT" \
  --user-data-dir="$PROFILE" \
  --no-first-run \
  --no-default-browser-check \
  about:blank
