# syntax=docker/dockerfile:1

# ── Frontend build stage ───────────────────────────────────
FROM oven/bun:1.3.12 AS frontend

WORKDIR /app/frontend
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile
COPY frontend/postcss.config.cjs frontend/svelte.config.js frontend/tailwind.config.ts frontend/tsconfig.json frontend/vite.config.ts ./
COPY frontend/public ./public
COPY frontend/src ./src
RUN bun run build

# ── Rust build stage ──────────────────────────────────────
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency build layer
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/

RUN cargo build --release --bin publaryn

# ── Runtime image ──────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/publaryn /usr/local/bin/publaryn
COPY --from=frontend /app/frontend/dist /app/static
COPY migrations/ migrations/

ENV SERVER__STATIC_DIR=/app/static

EXPOSE 3000
STOPSIGNAL SIGTERM

CMD ["publaryn"]
