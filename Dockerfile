# Staging context layout (created by scripts/fly-deploy.sh):
#   .fly-context/
#     Dockerfile  fly.toml  docker-entrypoint.sh
#     jobbot/ …
#     resuma/ …

# sqlx 0.9 / adk-rust need rustc ≥ 1.94
FROM rust:1.94-bookworm AS builder
WORKDIR /workspace

COPY resuma/Cargo.toml resuma/README.md ./resuma/
COPY resuma/crates/resuma-macros ./resuma/crates/resuma-macros
COPY resuma/crates/resuma ./resuma/crates/resuma
COPY resuma/client-sdk ./resuma/client-sdk
RUN python3 - <<'PY'
from pathlib import Path
import re
p = Path("resuma/Cargo.toml")
t = p.read_text()
t2, n = re.subn(
    r"members\s*=\s*\[[^\]]*\]",
    'members = [\n    "crates/resuma-macros",\n    "crates/resuma",\n]',
    t,
    count=1,
    flags=re.S,
)
if n != 1:
    raise SystemExit(f"failed to patch resuma workspace members (n={n})")
p.write_text(t2)
PY

COPY jobbot/Cargo.toml jobbot/Cargo.lock ./jobbot/
COPY jobbot/migrations ./jobbot/migrations
COPY jobbot/src ./jobbot/src
COPY jobbot/public ./jobbot/public

WORKDIR /workspace/jobbot
RUN cargo build --release --bin jobbot

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates gosu \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home jobbot \
    && mkdir -p /data/drafts /data/rate-limit \
    && chown -R jobbot:jobbot /data

WORKDIR /app
COPY --from=builder /workspace/jobbot/target/release/jobbot /app/jobbot
COPY --from=builder /workspace/jobbot/public /app/public
COPY --from=builder /workspace/jobbot/src/pages /app/pages
COPY --from=builder /workspace/jobbot/migrations /app/migrations
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
# Optional CV for drafts (applicant = Golfredo, never employer identity)
COPY cv/ /app/cv/
RUN chmod +x /app/docker-entrypoint.sh \
    && chown -R jobbot:jobbot /app

USER root

ENV RESUMA_ENV=production
ENV RESUMA_ADDR=0.0.0.0:8080
ENV RESUMA_TRUST_PROXY=1
ENV RESUMA_TRUSTED_PROXY_CIDRS=fdaa::/16
ENV RESUMA_PUBLIC_DIR=/app/public
ENV RESUMA_PAGES_ROOT=/app/pages
ENV RESUMA_DATA_DIR=/data
ENV DATABASE_URL=sqlite:/data/jobbot.db
ENV JOBBOT_AUTO_START=1
ENV JOBBOT_AUTO_APPLY=false
ENV JOBBOT_CV_PATH=/app/cv/CV_Golfredo_Perez_Tether_Backend.pdf
ENV RUST_LOG=info

EXPOSE 8080
ENTRYPOINT ["/app/docker-entrypoint.sh"]
