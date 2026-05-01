# ---- Build stage ----
FROM rust:bookworm AS builder

WORKDIR /app

# Cache dependencies before copying source
COPY Cargo.toml Cargo.lock ./
COPY common/Cargo.toml common/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
COPY bridge/Cargo.toml bridge/Cargo.toml

# Stub sources so Cargo can fetch + cache crate dependencies
RUN mkdir -p common/src server/src bridge/src \
    && echo "pub fn stub() {}" > common/src/lib.rs \
    && printf 'fn main() {}' > server/src/main.rs \
    && printf 'fn main() {}' > bridge/src/main.rs \
    && cargo fetch

# Copy real source and build
COPY common/ common/
COPY server/ server/
COPY bridge/ bridge/
COPY migrations/ migrations/

RUN cargo build --release -p remora-server -p remora-bridge

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash remora \
    && mkdir -p /var/lib/remora/workspaces \
    && chown remora:remora /var/lib/remora/workspaces

COPY --from=builder /app/target/release/remora-server /usr/local/bin/remora-server
COPY --from=builder /app/target/release/remora-bridge /usr/local/bin/remora-bridge

# Claude auth directory — mount from host: -v $HOME/.claude:/home/remora/.claude:ro
VOLUME ["/home/remora/.claude"]

USER remora

EXPOSE 7200

HEALTHCHECK --interval=10s --timeout=5s --start-period=20s --retries=6 \
    CMD curl -sf http://localhost:7200/health || exit 1

CMD ["/usr/local/bin/remora-server"]
