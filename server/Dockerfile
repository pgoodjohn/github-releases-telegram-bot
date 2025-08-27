FROM rust:1-bookworm AS builder

# System dependencies for building native deps (openssl, sqlite)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       pkg-config libssl-dev libsqlite3-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Leverage Docker layer caching: first copy manifests and do a dummy build
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true

# Now copy real sources and migrations (required at compile-time by sqlx::migrate!)
COPY src ./src
COPY migrations ./migrations

# Build the actual binary
RUN cargo build --release


FROM debian:bookworm-slim AS runtime

# Runtime libraries for native-tls and sqlite
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       libssl3 libsqlite3-0 ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from the builder image
COPY --from=builder /app/target/release/github-release-bot /usr/local/bin/github-release-bot

# Create non-root user for security
RUN useradd -m -u 10001 appuser
USER appuser

ENV RUST_LOG=info

CMD ["github-release-bot"]


