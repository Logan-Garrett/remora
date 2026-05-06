# Track 1: Auth and Identity

> Replace the shared team token with real user identity. This is the highest-priority track because it unblocks multi-tenancy (Track 2), the admin dashboard (Track 3), and audit logging.

---

## Current State

- One `REMORA_TEAM_TOKEN` shared across all users and sessions
- Display names are free-text strings chosen at connect time
- Trust is name-based (`session_trusted` table stores display names)
- Owner identity is proven via `owner_key` UUID, but there is no broader concept of "who are you"
- The impersonation window between disconnect/reconnect is documented in SECURITY.md

---

## Milestone 1: Per-Session Invite Tokens

**Priority: Highest** | **Depends on: nothing**

Scoped tokens that grant access to a single session, so sharing one session does not grant server-wide access.

### Implementation plan

1. Add a `session_tokens` table:
   ```
   session_tokens (
     id         UUID PRIMARY KEY,
     session_id UUID REFERENCES sessions(id),
     token      TEXT NOT NULL,       -- random 32-byte hex
     role       TEXT DEFAULT 'member', -- 'member' | 'viewer'
     created_by TEXT,
     created_at TIMESTAMPTZ,
     revoked    BOOLEAN DEFAULT FALSE
   )
   ```
2. New REST endpoint: `POST /sessions/:id/invite` -- returns a scoped token.
3. WebSocket upgrade accepts `session_token=<tok>` as an alternative to the global `token=<tok>`. When a session token is used, the connection is restricted to that session only.
4. The global `REMORA_TEAM_TOKEN` becomes an admin credential (can still access everything).
5. Migrations required for all three backends (Postgres, SQLite, MSSQL).

### Acceptance criteria

- A user with only a session token can join that session and no others
- A user with the admin token retains full access
- Revoking a session token disconnects active WebSocket connections using it
- Web client and Neovim plugin support passing session tokens

---

## Milestone 2: Per-Participant Invite Tokens

**Priority: High** | **Depends on: M1 (per-session tokens)**

Tie identity to a cryptographic token instead of a display name string.

### Implementation plan

1. Extend `session_tokens` with a `display_name` column. When a participant connects with a session token, their display name is determined by the token, not the query param.
2. Trust is granted to the token ID, not the name. Update `session_trusted` to reference the token:
   ```
   session_trusted (
     session_id       UUID,
     token_id         UUID REFERENCES session_tokens(id),
     participant_name TEXT,  -- denormalized for display
     added_at         TIMESTAMPTZ
   )
   ```
3. The `/trust` and `/untrust` commands resolve the target by display name but store the token ID internally.
4. Revoking a token automatically removes trust.

### Acceptance criteria

- The impersonation window documented in SECURITY.md is eliminated
- A participant reconnecting with the same token keeps their trusted status without re-granting
- Revoking a token revokes trust automatically

---

## Milestone 3: User Accounts (Built-in Auth)

**Priority: Medium** | **Depends on: M2**

Introduce persistent user identity stored in the database.

### Implementation plan

1. New tables:
   ```
   users (
     id            UUID PRIMARY KEY,
     email         TEXT UNIQUE NOT NULL,
     display_name  TEXT NOT NULL,
     password_hash TEXT NOT NULL,  -- argon2id
     created_at    TIMESTAMPTZ,
     role          TEXT DEFAULT 'member'  -- 'admin' | 'member'
   )

   user_sessions (
     user_id    UUID REFERENCES users(id),
     token      TEXT NOT NULL,     -- JWT or opaque session token
     expires_at TIMESTAMPTZ,
     created_at TIMESTAMPTZ
   )
   ```
2. New REST endpoints: `POST /auth/register`, `POST /auth/login`, `POST /auth/logout`.
3. Login returns a short-lived JWT. The JWT replaces the team token for all REST and WebSocket auth.
4. Display names are tied to accounts. The `name` query param on WebSocket is ignored when authenticated via JWT.
5. The admin role replaces the global team token for privileged operations (delete sessions, manage users).
6. Add password hashing dependency (`argon2` crate).

### Acceptance criteria

- Users can register and log in with email + password
- JWT-authenticated requests work for all REST and WebSocket endpoints
- The global team token still works as a backward-compatible admin credential
- Password reset flow exists (email or admin-initiated)

---

## Milestone 4: OAuth / SSO

**Priority: Medium-Low** | **Depends on: M3 (user accounts)**

Allow sign-in via external identity providers.

### Implementation plan

1. Add OAuth 2.0 authorization code flow for GitHub and Google as initial providers.
2. New table:
   ```
   oauth_connections (
     user_id      UUID REFERENCES users(id),
     provider     TEXT NOT NULL,  -- 'github' | 'google'
     provider_id  TEXT NOT NULL,
     access_token TEXT,           -- encrypted at rest
     created_at   TIMESTAMPTZ,
     UNIQUE(provider, provider_id)
   )
   ```
3. New REST endpoints: `GET /auth/oauth/:provider` (redirect), `GET /auth/oauth/:provider/callback`.
4. If a user with matching email already exists, link the OAuth connection. Otherwise, create a new account.
5. Server env vars: `REMORA_OAUTH_GITHUB_CLIENT_ID`, `REMORA_OAUTH_GITHUB_CLIENT_SECRET`, etc.
6. SAML/SSO can be added as a separate milestone if enterprise demand exists.

### Acceptance criteria

- Users can sign in with GitHub or Google
- OAuth-created accounts are full accounts (can also set a password for CLI auth)
- Multiple OAuth providers can be linked to one account

---

## Milestone 5: Role-Based Access Control

**Priority: Low** | **Depends on: M3 (user accounts)**

Enforce fine-grained permissions per user and per session.

### Roles

| Role | Capabilities |
|---|---|
| Admin | Full server access, manage users, set quotas, delete any session |
| Member | Create sessions, invite others, run Claude |
| Viewer | Read-only access to sessions they are invited to (no chat, no /run) |
| Guest | Join a single session via invite token, no session creation |

### Implementation plan

1. Add `role` column to `users` table (already anticipated in M3).
2. Add `session_members` table mapping users to sessions with per-session roles.
3. Enforce role checks in `commands.rs` dispatch and REST handlers in `lib.rs`.
4. Viewer role: allow WebSocket subscription and backfill but reject all `ClientMsg` variants except `Who` and `Help`.

### Acceptance criteria

- Admin can manage all sessions and users
- Viewer can read but not write
- Guest can only access the single session they were invited to
- Role checks are enforced server-side (not just UI-hidden)

---

## Dependency Graph

```
M1 (Per-session tokens)
  └── M2 (Per-participant tokens)
        └── M3 (User accounts)
              ├── M4 (OAuth / SSO)
              └── M5 (RBAC)
```

M1 is standalone and can ship immediately. Each subsequent milestone builds on the previous one. M4 and M5 are independent of each other and can be developed in parallel once M3 is complete.

---

## Risks and Open Questions

- **Migration complexity**: Adding auth to a running server requires a transition period where both old (team token) and new (JWT) auth work simultaneously. Plan for at least one release where both are supported.
- **Password storage**: The server currently has no crypto dependencies beyond `subtle` (constant-time compare). Adding `argon2` is a new dependency.
- **Web client changes**: The login flow in `web/src/login.ts` needs to support both token-based and account-based auth during the transition.
- **Bridge binary**: `remora-bridge` passes the token as a query param. It needs to support JWT auth or API keys for non-interactive use.
