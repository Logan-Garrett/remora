# Track 2: Infrastructure and Scaling

> Expand database support, enable multi-instance deployments, improve deployment tooling, and handle session lifecycle better. This track makes Remora production-ready for teams beyond a single Raspberry Pi.

---

## Current State

- Three database backends: Postgres (LISTEN/NOTIFY), SQLite (in-process broadcast), MSSQL (in-process broadcast)
- Single-instance only for SQLite and MSSQL; Postgres supports multi-instance via LISTEN/NOTIFY but `participants` map is still per-process
- Docker sandbox isolation exists but network is fully blocked (`--network none`)
- `docker-compose.yml` exists for local dev (Postgres + server + nginx)
- Cross-compile for ARM64 works via `cargo zigbuild`
- Idle session cleanup deletes workspace but retains event history in DB
- No session export or workspace archival before cleanup

---

## Milestone 1: Multi-Instance Participant Presence

**Priority: Highest** | **Depends on: nothing**

The `participants` and `session_owners` HashMaps in `AppState` are per-process. With multiple server instances behind a load balancer, `/who` only shows users connected to the current instance, and ownership can be split.

### Implementation plan

1. Move participant presence to the database:
   ```
   session_participants (
     session_id   UUID REFERENCES sessions(id),
     name         TEXT NOT NULL,
     instance_id  TEXT NOT NULL,  -- identifies the server process
     connected_at TIMESTAMPTZ,
     UNIQUE(session_id, name)
   )
   ```
2. On WebSocket connect, insert a row. On disconnect, delete it.
3. `/who` queries the table instead of the in-memory map.
4. Heartbeat: each instance updates a `last_seen` column periodically. A reaper deletes stale rows where `last_seen` is older than 60s (handles ungraceful shutdowns).
5. `session_owners` can remain in-memory as long as `owner_key` is the primary ownership mechanism (it is DB-backed already). For multi-instance, ownership should also be queried from the DB.

### Acceptance criteria

- `/who` shows participants across all instances
- Display name uniqueness is enforced across instances (DB unique constraint)
- Ungraceful instance shutdown does not leave phantom participants permanently

---

## Milestone 2: Redis Pub/Sub Notification Backend

**Priority: High** | **Depends on: M1**

Add Redis as a notification transport so SQLite users can run multiple instances, and Postgres users get a lighter-weight alternative to LISTEN/NOTIFY.

### Implementation plan

1. New `DatabaseBackend` variant or a separate notification layer (since Redis is not a DB backend, consider splitting notification from storage).
2. `REMORA_NOTIFY_BACKEND=redis` env var, with `REDIS_URL`.
3. Implement `subscribe_notifications()` using Redis pub/sub on a channel per session.
4. The `run_event_listener` in `state.rs` already abstracts over the notification source -- this plugs in cleanly.
5. Add `redis` crate dependency (behind a feature flag like `mssql`).

### Acceptance criteria

- Two server instances with SQLite + Redis can fan out events to each other's clients
- Redis connection loss triggers reconnect with backoff
- Feature-gated so builds without Redis don't pull in the dependency

---

## Milestone 3: MySQL / MariaDB Backend

**Priority: Medium** | **Depends on: nothing**

The fourth database backend. MySQL is the most common self-hosted DB and is a frequent request.

### Implementation plan

1. Implement the `Database` trait for MySQL in `server/src/db/mysql.rs`.
2. Add migrations in `migrations/mysql/`.
3. `REMORA_DB_PROVIDER=mysql` with `DATABASE_URL=mysql://...`.
4. Use `sqlx` MySQL feature (already supports it).
5. Notification: in-process broadcast (same as SQLite/MSSQL) unless Redis is also configured.
6. Add CI job: `test-mysql` using `mysql:8` service container.

### Acceptance criteria

- Full test suite passes against MySQL 8
- All existing migrations have MySQL equivalents
- CI runs MySQL tests on every push

---

## Milestone 4: Session Export and Workspace Archival

**Priority: Medium** | **Depends on: nothing**

Before idle cleanup destroys a workspace, preserve the work.

### Implementation plan

1. **Event log export**: On session expiry, generate a Markdown transcript of all events and store it as a blob in a new `session_exports` table (or write to a configured path).
2. **Git push on cleanup**: Before deleting workspace repos, push all branches to their remotes. Requires that the server has push credentials (SSH key or token). Skip if push fails -- log a warning but don't block cleanup.
3. **Object storage archival** (optional): If `REMORA_ARCHIVE_URL` is set (S3-compatible endpoint), tar the workspace and upload before deletion.
4. New env vars: `REMORA_EXPORT_ON_CLEANUP=true`, `REMORA_ARCHIVE_URL`, `REMORA_ARCHIVE_BUCKET`.

### Acceptance criteria

- Event transcripts are preserved after session cleanup
- Git repos are pushed before workspace deletion (best-effort)
- Object storage upload works with S3-compatible endpoints (tested with MinIO)

---

## Milestone 5: Deployment Tooling

**Priority: Medium-Low** | **Depends on: nothing**

Make deploying Remora easier for operators.

### Items

1. **Pre-built Docker images on GHCR**: The `release.yml` workflow already builds images. Ensure they are multi-arch (amd64 + arm64) and tagged with both the version and `latest`.
2. **Helm chart**: A `deploy/helm/remora/` chart with configurable values for database, token, resource limits, and ingress. Supports Postgres (recommended) and SQLite (single-replica).
3. **Improved docker-compose.yml**: Add optional Redis service, healthchecks, volume configuration documentation.
4. **Systemd unit file**: A `deploy/remora-server.service` template for bare-metal deployments (the current `start.sh`/`stop.sh` pattern works but systemd is more robust).
5. **Terraform module** (stretch): For cloud deployments on AWS/GCP (ECS/Cloud Run + RDS/Cloud SQL).

### Acceptance criteria

- `helm install remora ./deploy/helm/remora` works with a Postgres database
- Systemd unit file starts/stops/restarts the server correctly
- Docker images are available for both amd64 and arm64

---

## Milestone 6: Sandbox Networking and Egress Control

**Priority: Low** | **Depends on: nothing**

The Docker sandbox currently runs with `--network none`. This blocks Claude from fetching URLs or cloning repos inside the sandbox.

### Implementation plan

1. Add a sandboxed network with a transparent proxy that enforces the session's fetch allowlist.
2. Use a lightweight proxy (e.g., `squid` or a custom Go/Rust proxy) that checks each outbound request against `session_allowlist` and `global_allowlist`.
3. The proxy runs as a sidecar container in the sandbox network.
4. New env var: `REMORA_SANDBOX_NETWORK=filtered` (default remains `none`).

### Acceptance criteria

- Claude inside the sandbox can fetch URLs that are on the session/global allowlist
- Requests to non-allowlisted domains are blocked and logged
- `--network none` remains the default for maximum isolation

---

## Dependency Graph

```
M1 (Participant presence in DB)
  └── M2 (Redis pub/sub)

M3 (MySQL backend)           -- independent
M4 (Session export)          -- independent
M5 (Deployment tooling)      -- independent
M6 (Sandbox networking)      -- independent
```

M1 and M2 are sequential. Everything else is independent and can be tackled in any order based on demand.

---

## Risks and Open Questions

- **Participant presence in DB**: Adds a write on every connect/disconnect. For SQLite this could cause lock contention under high connection churn. May need WAL mode enforcement or a separate presence store.
- **Redis dependency**: Adding Redis is a significant operational burden for small deployments. It should always be optional, never required.
- **MySQL type differences**: MySQL's UUID handling differs from Postgres (no native UUID type). The `sessions.id` column will need `CHAR(36)` or `BINARY(16)` with conversion logic.
- **Sandbox networking**: The proxy approach adds complexity. An alternative is to use Docker's built-in firewall rules (`--iptables`) to allow specific domains, but that requires root and is less portable.
