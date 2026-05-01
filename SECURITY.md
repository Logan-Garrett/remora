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

## Known Limitations

These are documented design decisions, not bugs:

- **WebSocket token in query string** — The team token is passed as `?token=...` during the WebSocket upgrade. The browser `WebSocket` API cannot set headers, so this is standard practice. The token may appear in reverse proxy access logs. Configure your proxy to strip query strings, or rotate the token periodically.

- **Single shared team token** — Knowing the team token grants access to all sessions on the server. Per-session scoped tokens are on the [roadmap](ROADMAP.md).

- **`--dangerously-skip-permissions` is on by default** — Claude runs with full permissions on the server host unless `REMORA_SKIP_PERMISSIONS=false` is set, or `REMORA_USE_SANDBOX=true` is used to isolate Claude in a Docker container per session. Only run Remora on hosts you trust, with a token you keep secret.

- **Prompt injection** — Chat messages from session participants are passed as context to Claude. A malicious participant could craft a message designed to influence Claude's behavior. Treat the team token and session access as privileged.

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

There is a brief window between a trusted participant disconnecting and reconnecting where their display name is available. During this window, an attacker who knows the name could theoretically connect with it and have their messages treated as trusted. This window is typically sub-second for reconnects, but it exists.

### Future mitigation

Per-participant invite tokens (see [ROADMAP.md](ROADMAP.md), Phase 1) will eliminate name-based impersonation entirely by tying identity to a cryptographic token rather than a display name string.

More broadly, the shared team token is the weakest point in the security model — it provides no identity, no audit trail, and no per-user revocation. A dedicated auth service (built-in JWT-based or external OAuth/SSO) is planned for Phase 1 to replace token-based auth with verified identities. See the [roadmap](ROADMAP.md) for details.
