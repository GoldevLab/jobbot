# JobBot — build from this repository (Resuma pulled via git dependency).
# sqlx 0.9 / adk-rust need rustc ≥ 1.94
FROM rust:1.94-bookworm AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Do not copy .cargo/config.toml — CI/Fly must use the git dep, not a local path patch.
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
COPY public ./public

RUN cargo build --release --bin jobbot

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates gosu \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home jobbot \
    && mkdir -p /data/drafts /data/rate-limit \
    && chown -R jobbot:jobbot /data

WORKDIR /app
COPY --from=builder /app/target/release/jobbot /app/jobbot
COPY --from=builder /app/public /app/public
COPY --from=builder /app/src/pages /app/pages
COPY --from=builder /app/migrations /app/migrations
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
# Optional CV (empty dir is fine in CI; local deploy can place a PDF in cv/)
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
ENV JOBBOT_AUTO_APPLY=true
ENV JOBBOT_CV_PATH=/app/cv/CV_Golfredo_Perez_Tether_Backend.pdf
ENV RUST_LOG=info

EXPOSE 8080
ENTRYPOINT ["/app/docker-entrypoint.sh"]
