# ─────────────────────────────────────────────────────────────────────────────
# Stage 1 – Build
# ─────────────────────────────────────────────────────────────────────────────
FROM rust:1.95-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies (needed for linking even with bundled SQLite)
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from source compilation.
# Copy only the manifest first, build a dummy binary, then replace with real src.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs
RUN cargo build --release 2>&1 | tail -5
RUN rm -f src/main.rs

# Now copy real source and rebuild (only the app crate recompiles)
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ─────────────────────────────────────────────────────────────────────────────
# Stage 2 – Runtime
# ─────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

WORKDIR /app

# ca-certificates for outbound TLS (SNS HTTPS subscriptions)
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cloudtwin /app/cloudtwin

# Dashboard static assets + default config
COPY dashboard/static /app/dashboard/static
COPY config/cloudtwin.yml /config/cloudtwin.yml

VOLUME ["/data"]
EXPOSE 4793
# Built-in SMTP listener (capture mailbox + SES receiving)
EXPOSE 1025

ENV CLOUDTWIN_DB_PATH=/data/cloudtwin.db
ENV CLOUDTWIN_DASHBOARD_STATIC=/app/dashboard/static
ENV CLOUDTWIN_CONFIG_PATH=/config/cloudtwin.yml

CMD ["/app/cloudtwin"]
