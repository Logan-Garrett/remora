# ---- Build stage ----
# pinned via tag, digest rotates
FROM rust:bookworm@sha256:adab7941580c74513aa3347f2d2a1f975498280743d29ec62978ba12e3540d3a AS builder

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

# ---- Runtime stage (pinned via tag, digest rotates) ----
FROM debian:bookworm-slim@sha256:f9c6a2fd2ddbc23e336b6257a5245e31f996953ef06cd13a59fa0a1df2d5c252

# Install runtime deps + Node.js 20 (for Claude CLI)
# Uses NodeSource — NOT the Debian split packages (which pull 400+ deps)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gnupg \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force \
    && apt-get purge -y gnupg \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/* /tmp/* /root/.npm

RUN useradd -m -s /bin/bash remora \
    && mkdir -p /var/lib/remora/workspaces \
    && chown remora:remora /var/lib/remora/workspaces

COPY --from=builder /app/target/release/remora-server /usr/local/bin/remora-server
COPY --from=builder /app/target/release/remora-bridge /usr/local/bin/remora-bridge

# Mount host Claude auth directory so the CLI can authenticate:
#   -v $HOME/.claude:/home/remora/.claude:ro
VOLUME ["/home/remora/.claude"]

USER remora

EXPOSE 7200

HEALTHCHECK --interval=10s --timeout=5s --start-period=20s --retries=6 \
    CMD curl -sf http://localhost:7200/health || exit 1

CMD ["/usr/local/bin/remora-server"]
