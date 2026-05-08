# Remora VS Code Extension

Collaborative Claude Code sessions directly in VS Code. Connects to a [Remora](https://github.com/logangarrett03/remora) server over native WebSocket -- no bridge binary needed.

## Install

```bash
cd vscode
npm install
npm run build
```

To package as a `.vsix` for local install:

```bash
npx @vscode/vsce package
code --install-extension remora-0.9.3.vsix
```

## Setup

Open VS Code settings and configure:

- `remora.serverUrl` -- your Remora server URL (e.g. `http://localhost:7200`)
- `remora.token` -- team authentication token
- `remora.displayName` -- your name in sessions

Or leave them blank and the extension will prompt on first connect.

## Commands

| Command | Description |
|---|---|
| `Remora: Connect to Server` | Set server URL, token, and display name |
| `Remora: List Sessions` | Browse and join existing sessions |
| `Remora: Join Session` | Join a session (or create one if none exist) |
| `Remora: Leave Session` | Disconnect from the current session |
| `Remora: Send Message` | Send a chat message or slash command |

## Features

- Activity bar icon with dedicated chat panel
- Tokyo Night dark theme matching the web client
- Full slash command support (`/run`, `/who`, `/help`, `/diff`, `/add`, etc.)
- Streaming Claude responses with live updates
- Tool call and result display
- Auto-reconnect on disconnect (up to 3 attempts)
- Status bar showing connection state and current session

## Slash Commands

All commands from the web client are supported:

```
/help        Show available commands
/run         Start a Claude run
/run-all     Run Claude with all context
/who         List connected participants
/info        Show session info
/diff        Show current diff
/add <path>  Add a file to context
/fetch <url> Fetch a URL into context
/kick <name> Kick a participant (owner only)
/trust <name>   Grant trust (owner only)
/untrust <name> Revoke trust (owner only)
/repo add <url>       Add a git repo
/repo remove <name>   Remove a git repo
/repo list            List repos
/allowlist add <domain>    Add allowed domain
/allowlist remove <domain> Remove allowed domain
```
