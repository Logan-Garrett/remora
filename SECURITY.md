# Security Policy

## Supported Versions

Security fixes are applied to the latest release only. If you are running an older version, please upgrade.

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report vulnerabilities privately by emailing the maintainer directly, or by using [GitHub's private vulnerability reporting](https://github.com/Logan-Garrett/remora/security/advisories/new).

Include:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept (if possible)
- The version/commit you tested against

You can expect an acknowledgement within 48 hours and a resolution or mitigation plan within 14 days.

## Auth Model

Remora uses a layered authentication system with multiple credential types resolved in priority order:

### Password hashing

User passwords are hashed with **argon2** (the recommended memory-hard KDF). The default Argon2 configuration is used. Password hashes are stored in the `users` table and never exposed through any API endpoint.

### JWT lifecycle

- **Access tokens** are short-lived JWTs (default 1 hour, configurable via `REMORA_JWT_EXPIRY_SECS`). They contain the user ID, display name, and role. Signed with HMAC-SHA256 using `REMORA_JWT_SECRET`.
- **Refresh tokens** are long-lived random strings (default 7 days). The raw token is returned to the client; only the SHA-256 hash is stored in the database.
- **Refresh token rotation**: every refresh request atomically consumes the old token (DELETE ... RETURNING in a single query) and issues a new one. This prevents race conditions where two concurrent requests could both validate the same token.
- If `REMORA_JWT_SECRET` is not set, the server generates a random UUID on startup and logs a warning. Tokens will not survive restarts.

### OAuth

GitHub and Google OAuth 2.0 are supported. Two flows are available depending on the calling context:

**Popup flow (web client):**
1. The web client opens the redirect endpoint in a popup window, passing `?origin=<web-client-origin>`.
2. The server encodes the origin into the CSRF `state` parameter (UUID + HMAC signature using the JWT secret). This is self-validating, requiring no server-side storage.
3. The callback endpoint validates the `state` signature and extracts the origin before exchanging the authorization code.
4. On success, the server returns an HTML page that calls `window.opener.postMessage(authData, origin)` targeted to the exact origin, then closes the popup. The web client validates `event.origin` matches the server origin before accepting the message.

**Non-browser flow (CLI, integrations):**
1. The redirect endpoint generates a CSRF `state` parameter (UUID + HMAC signature using the JWT secret). This is self-validating, requiring no server-side storage.
2. The callback endpoint validates the `state` signature before exchanging the authorization code.
3. On success, the server returns a JSON `AuthResponse`, or redirects to `REMORA_OAUTH_REDIRECT_URL` with the JWT as a query parameter if that env var is set.

In both flows: if the OAuth provider email matches an existing account, the OAuth connection is linked to that account. Otherwise, a new account is created.

### API key hashing

API keys are prefixed with `rmk_` for identification. Only the SHA-256 hash is stored in the database. The raw key is returned once at creation time and cannot be retrieved later.

### Role-based access

Four roles with a numeric hierarchy:

| Role | Level | Access |
|---|---|---|
| Admin | 4 | Full server access |
| Member | 3 | Create sessions, invite others, use Claude |
| Viewer | 2 | Read-only access |
| Guest | 1 | Single session access via invite token |

Role enforcement helpers (`role_level()`, `require_role()`) are implemented in `auth.rs`. RBAC enforcement in WebSocket command dispatch is planned but not yet wired in (documented as TODO in `commands.rs`).

### Token resolution order

The `check_any_token()` function in `lib.rs` resolves credentials in order:
1. Admin team token (constant-time comparison)
2. JWT (decoded and user looked up from DB)
3. Session invite token (DB lookup, scoped to a single session)
4. API key (SHA-256 hash lookup)

## Team Isolation

Sessions can optionally belong to a team. When a session has a `team_id`, the server enforces that only team members can access it. Isolation is checked at two points:

### REST endpoints
All team endpoints (`/teams/:id`, `/teams/:id/members`, `/teams/:id/sessions`) verify that the authenticated user is a member of the team before returning data. Non-members receive `403 Forbidden`.

### WebSocket upgrade
When a client connects to a team-scoped session via WebSocket, the server looks up the session's `team_id`. If the session belongs to a team, the server checks that the connecting user (identified by JWT or API key) is a member of that team. Non-members receive `403 Forbidden` and the connection is rejected before upgrade.

### Bypasses
Two credential types bypass the team membership check:
- **Admin token** (`REMORA_TEAM_TOKEN`): the server-level admin token has full access to all sessions regardless of team ownership.
- **Session invite tokens**: these are already scoped to a single session and do not carry user identity, so they bypass the team check by design.

### Team deletion behavior
When a team is deleted, its sessions are **detached** (the `team_id` column is set to NULL) rather than deleted. This prevents accidental data loss. Detached sessions become unscoped and are accessible via the standard admin-token-authenticated endpoints.

### Role enforcement within teams
Team member roles (`admin`, `member`, `viewer`) control what actions a user can take within the team. Admins can manage members and team settings. Members can create team sessions. Viewers have read access to team data. Role checks happen at the handler level before any DB mutation.

## Known Limitations

These are documented design decisions, not bugs:

- **WebSocket token in query string** -- The token is passed as `?token=...` during the WebSocket upgrade. The browser `WebSocket` API cannot set headers, so this is standard practice. The token may appear in reverse proxy access logs. Configure your proxy to strip query strings, or rotate the token periodically.

- **RBAC not enforced in WebSocket commands** -- Role checks exist as helpers but are not yet integrated into the command dispatch pipeline. A viewer can currently execute any command once connected. This is tracked as a TODO in `commands.rs`.

- **Permissive CORS** -- The server uses `CorsLayer::permissive()`. This is a pre-existing configuration that predates the auth system. It should be tightened to specific origins in production. The OAuth postMessage flow uses targeted origin validation (`postMessage(data, origin)`) to mitigate this for OAuth callbacks specifically.

- **`--dangerously-skip-permissions` is on by default** -- Claude runs with full permissions on the server host unless `REMORA_SKIP_PERMISSIONS=false` is set, or `REMORA_USE_SANDBOX=true` is used to isolate Claude in a Docker container per session. Only run Remora on hosts you trust, with a token you keep secret.

- **Prompt injection** -- Chat messages from session participants are passed as context to Claude. A malicious participant could craft a message designed to influence Claude's behavior. Treat session access as privileged.

## Trust Model

Remora implements a name-based trust system for controlling which participants' messages reach Claude as direct instructions vs. untrusted content.

### How it works

- **Display names are unique per session.** When a participant connects via WebSocket, the server rejects the connection if someone with the same display name is already connected to that session. This prevents impersonation while a participant is actively connected.

- **Trust is name-based and DB-persisted.** The `/trust <name>` command adds a participant's display name to the `session_trusted` table. Trusted participants' chat messages are sent to Claude as plain instructions (`[name (trusted)]: ...`). Untrusted participants' messages are wrapped in `<untrusted_content>` tags, which Claude treats as user-supplied data rather than instructions.

- **Only the session owner can grant or revoke trust.** The session owner is the first participant to join a session after the server starts (or after the session is created). Only the owner can run `/trust` and `/untrust`. Other participants who attempt these commands receive an error.

- **Trust persists across reconnects.** The trusted list is stored in the database and survives server restarts. The session owner designation is in-memory only and resets when the server restarts (the first person to rejoin becomes the new owner).

### Owner key

When a session is created, the server generates a random `owner_key` (UUID) and stores it in the database. The creator receives this key in the REST response. To claim persistent ownership of a session, include `owner_key=<key>` in the WebSocket query parameters. This replaces the fragile "first to join is owner" logic with cryptographic proof stored in the DB:

- Connecting with a **valid** `owner_key` makes you the session owner, overriding any existing in-memory owner.
- Connecting with an **invalid** `owner_key` (or none at all) falls back to the previous first-joiner behavior.
- The `owner_key` persists across server restarts since it is stored in the database.
- The `owner_key` is only returned once (in the create-session response) and is never included in list responses. Treat it as a secret.

### Known limitation

There is a brief window between a trusted participant disconnecting and reconnecting where their display name is available. During this window, an attacker who knows the name could theoretically connect with it and have their messages treated as trusted. This window is typically sub-second for reconnects, but it exists. Once RBAC enforcement is fully wired in, trust will be tied to authenticated user identity rather than display name alone.
