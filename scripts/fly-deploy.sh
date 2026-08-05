#!/usr/bin/env bash
# Deploy JobBot to Fly.io from this repo (same path as GitHub Actions).
# Fly account = hosting only. Applicant identity = Golfredo (DB defaults / CV).
set -euo pipefail

APP="${FLY_APP:-golfredo-jobbot}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGION="${FLY_REGION:-gru}"
VOLUME_NAME="${FLY_VOLUME_NAME:-jobbot_data}"
CV_SRC="${JOBBOT_CV_SRC:-/home/golfredo/Documentos/CV_Golfredo_Perez_Tether_Backend.pdf}"

if command -v fly >/dev/null 2>&1; then
  FLY=(fly)
elif command -v flyctl >/dev/null 2>&1; then
  FLY=(flyctl)
else
  echo "Instala flyctl: https://fly.io/docs/hands-on/install-flyctl/"
  exit 1
fi

if ! "${FLY[@]}" auth whoami >/dev/null 2>&1; then
  echo "Ejecuta: fly auth login (o define FLY_API_TOKEN)"
  exit 1
fi

FLY_USER="$("${FLY[@]}" auth whoami 2>/dev/null | head -1 || true)"
echo "==> Fly account (hosting only): ${FLY_USER:-unknown}"
echo "    Applicant identity in-app: Golfredo Pérez / golfredo.pf@gmail.com"

cd "$ROOT"

if [[ ! -f Cargo.lock ]]; then
  echo "==> Generando Cargo.lock"
  cargo generate-lockfile
fi

mkdir -p cv
if [[ -f "$CV_SRC" ]]; then
  cp -a "$CV_SRC" "cv/CV_Golfredo_Perez_Tether_Backend.pdf"
  echo "    CV bundled into cv/ for this deploy"
else
  echo "    ⚠ CV not found at $CV_SRC — continuing without PDF"
fi

echo "==> App $APP"
if ! "${FLY[@]}" status -a "$APP" >/dev/null 2>&1; then
  echo "    Creating app…"
  "${FLY[@]}" apps create "$APP" --org personal
fi

if ! "${FLY[@]}" volumes list -a "$APP" 2>/dev/null | grep -q "$VOLUME_NAME"; then
  echo "==> Creating volume $VOLUME_NAME ($REGION, 1GB)"
  "${FLY[@]}" volumes create "$VOLUME_NAME" --region "$REGION" --size 1 -a "$APP" -y
fi

ENV_FILE="$ROOT/.env"
if [[ -f "$ENV_FILE" ]]; then
  KEY_LINE="$(grep -E '^OPENROUTER_API_KEY=' "$ENV_FILE" | head -1 || true)"
  if [[ -n "$KEY_LINE" ]]; then
    OR_KEY="${KEY_LINE#OPENROUTER_API_KEY=}"
    OR_KEY="${OR_KEY%\"}"
    OR_KEY="${OR_KEY#\"}"
    if [[ -n "$OR_KEY" ]]; then
      echo "==> Setting OPENROUTER_API_KEY secret"
      "${FLY[@]}" secrets set -a "$APP" "OPENROUTER_API_KEY=${OR_KEY}" --stage
    fi
  else
    echo "    ⚠ OPENROUTER_API_KEY missing in .env"
  fi
else
  echo "    ⚠ No .env — set: fly secrets set -a $APP OPENROUTER_API_KEY=…"
fi

echo "==> Deploy (same as GitHub Action: fly deploy --remote-only)"
"${FLY[@]}" deploy --remote-only --app "$APP" "$@"

echo ""
echo "Listo: https://${APP}.fly.dev/"
echo "Health: https://${APP}.fly.dev/health"
echo "CD: push to main → .github/workflows/fly.yml"
