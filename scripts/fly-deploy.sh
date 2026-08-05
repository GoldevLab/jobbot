#!/usr/bin/env bash
# Deploy JobBot to Fly.io (always-on worker).
# Fly account = hosting only. Applicant identity = Golfredo (DB defaults / CV).
set -euo pipefail

APP="${FLY_APP:-golfredo-jobbot}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APPS="$(cd "$ROOT/.." && pwd)"
CTX="$ROOT/.fly-context"
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
echo "    Applicant identity in-app: Golfredo Pérez / golfredo.pf@gmail.com (not employer email)"

if [[ ! -d "$APPS/resuma/crates/resuma" ]]; then
  echo "Falta $APPS/resuma (path dependency de jobbot)."
  exit 1
fi

if [[ ! -f "$ROOT/Cargo.lock" ]]; then
  echo "==> Generando jobbot/Cargo.lock"
  (cd "$ROOT" && cargo generate-lockfile)
fi

echo "==> Staging context en $CTX"
rm -rf "$CTX"
mkdir -p "$CTX/jobbot" "$CTX/resuma/crates" "$CTX/cv"

rsync -a --delete \
  --exclude target --exclude .resuma --exclude .fly-context \
  --exclude node_modules --exclude data \
  --exclude '.env' --exclude '.env.*' \
  "$ROOT/" "$CTX/jobbot/"

cp -a "$APPS/resuma/Cargo.toml" "$CTX/resuma/"
if [[ -f "$APPS/resuma/Cargo.lock" ]]; then
  cp -a "$APPS/resuma/Cargo.lock" "$CTX/resuma/"
elif command -v cargo >/dev/null 2>&1; then
  echo "==> Generando resuma/Cargo.lock"
  (cd "$APPS/resuma" && cargo generate-lockfile)
  cp -a "$APPS/resuma/Cargo.lock" "$CTX/resuma/"
fi
cp -a "$APPS/resuma/README.md" "$CTX/resuma/" 2>/dev/null || printf '# resuma\n' > "$CTX/resuma/README.md"
rsync -a --delete --exclude target "$APPS/resuma/crates/resuma-macros/" "$CTX/resuma/crates/resuma-macros/"
rsync -a --delete --exclude target "$APPS/resuma/crates/resuma/" "$CTX/resuma/crates/resuma/"
rsync -a --delete "$APPS/resuma/client-sdk/" "$CTX/resuma/client-sdk/" 2>/dev/null || mkdir -p "$CTX/resuma/client-sdk"

if [[ -f "$CV_SRC" ]]; then
  cp -a "$CV_SRC" "$CTX/cv/CV_Golfredo_Perez_Tether_Backend.pdf"
  echo "    CV: Golfredo PDF bundled"
else
  echo "    ⚠ CV no encontrado en $CV_SRC — drafts without attach path"
  # Empty dir still OK for COPY cv/
  : > "$CTX/cv/.keep"
fi

cp -f "$ROOT/Dockerfile" "$CTX/Dockerfile"
cp -f "$ROOT/fly.toml" "$CTX/fly.toml"
cp -f "$ROOT/.dockerignore" "$CTX/.dockerignore"
cp -f "$ROOT/docker-entrypoint.sh" "$CTX/docker-entrypoint.sh"
chmod +x "$CTX/docker-entrypoint.sh"

echo "==> App $APP"
if ! "${FLY[@]}" status -a "$APP" >/dev/null 2>&1; then
  echo "    Creando app…"
  "${FLY[@]}" apps create "$APP" --org personal
fi

if ! "${FLY[@]}" volumes list -a "$APP" 2>/dev/null | grep -q "$VOLUME_NAME"; then
  echo "==> Creando volume $VOLUME_NAME ($REGION, 1GB)"
  "${FLY[@]}" volumes create "$VOLUME_NAME" --region "$REGION" --size 1 -a "$APP" -y
fi

# OpenRouter secret from local .env (never commit). Does not set employer emails.
ENV_FILE="$ROOT/.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  set -a
  # Only pull the key we need — ignore unrelated vars
  KEY_LINE="$(grep -E '^OPENROUTER_API_KEY=' "$ENV_FILE" | head -1 || true)"
  set +a
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

echo "==> Deploy"
cd "$CTX"
"${FLY[@]}" deploy . --config fly.toml --dockerfile Dockerfile --app "$APP" "$@"

echo ""
echo "Listo: https://${APP}.fly.dev/"
echo "Health: https://${APP}.fly.dev/health"
echo "Worker: JOBBOT_AUTO_START=1 · discover/score/draft 24/7 (apply off on Fly)"
echo "Applicant: Golfredo (settings) — Fly login is hosting only"
