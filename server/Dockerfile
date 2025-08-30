FROM rust:1-bookworm AS chef

WORKDIR /app

# Install cargo-chef to compute dependency graph
RUN cargo install cargo-chef --locked

# Prepare the recipe for caching dependency builds
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM rust:1-bookworm AS builder

# System dependencies for building native deps (openssl, sqlite)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       pkg-config libssl-dev libsqlite3-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install cargo-chef in the builder and cook deps using the recipe
RUN cargo install cargo-chef --locked
COPY --from=chef /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

# Now copy the full source and build the actual binary.
# Note: migrations are required at compile-time by sqlx::migrate!
COPY . .
RUN cargo build --release --locked


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


