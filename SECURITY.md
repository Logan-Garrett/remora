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
