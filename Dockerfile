# syntax=docker/dockerfile:1
FROM rust:1.77-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency build layer
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin publaryn

# ── Runtime image ──────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/publaryn /usr/local/bin/publaryn
COPY migrations/ migrations/

EXPOSE 3000
STOPSIGNAL SIGTERM

CMD ["publaryn"]
