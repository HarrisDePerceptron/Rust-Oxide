# syntax=docker/dockerfile:1.7

FROM rust:1.88-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --locked --profile release-server -p rust_oxide \
    && cp /app/target/release-server/rust_oxide /tmp/rust_oxide

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /app appuser

WORKDIR /app

COPY --from=builder /tmp/rust_oxide /usr/local/bin/rust_oxide
COPY --from=builder /app/crates/server/public /app/public

ENV APP_GENERAL__HOST=0.0.0.0 \
    APP_GENERAL__PORT=3000 \
    APP_PUBLIC_DIR=/app/public

EXPOSE 3000

USER appuser:appuser

CMD ["/usr/local/bin/rust_oxide"]
