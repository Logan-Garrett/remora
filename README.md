# Remora

Collaborative Claude Code sessions. Multiple devs share a single session with a shared, append-only event log. Chat freely, add context, and invoke Claude together.

## Screenshots

### Session Picker (`<space>ms`)
Browse and join sessions with Telescope.

![Session Picker](assets/session-picker.svg)

### Shared Chat Window
Real-time event log with syntax highlighting. Everyone sees chat, tool calls, and Claude's responses as they happen.

![Chat Window](assets/chat-window.svg)

### Command Picker (`<space>mc`)
Fuzzy-searchable command palette for all Remora actions.

![Command Picker](assets/command-picker.svg)

## How It Works

1. **Create a session** — one dev creates a session (optionally with git repos cloned in)
2. **Everyone joins** — teammates connect by session ID with `<space>ms`
3. **Chat and add context** — send messages, inline files (`/add`), fetch URLs (`/fetch`), view diffs (`/diff`)
4. **Invoke Claude** — anyone types `/run` and Claude sees all context since the last response
5. **Watch Claude work** — tool calls, file edits, and responses stream to everyone in real-time
6. **Iterate** — keep chatting, adding context, and running Claude as a team

Everything is persisted in Postgres. Reconnect anytime and get the full history.

## Architecture

![Architecture](assets/architecture.svg)

- **Neovim Plugin** — Lua plugin with Telescope integration. Communicates via a small Rust bridge binary over WebSocket.
- **Server** — Rust (axum). Handles auth, WebSocket connections, event fan-out via Postgres LISTEN/NOTIFY. Stateless across restarts.
- **Postgres** — Append-only event log, session metadata, allowlists, quotas. All state lives here.
- **Claude CLI** — Invoked directly on the server host with `--dangerously-skip-permissions`. Streams output as events.

## Setup

### 1. Server

Requires Postgres and the [Claude CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated.

```bash
# Create database
sudo -u postgres psql -c "CREATE USER remora WITH PASSWORD 'your-password';"
sudo -u postgres psql -c "CREATE DATABASE remora OWNER remora;"
sudo -u postgres psql -c "\c remora" -c "CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\";"
sudo -u postgres psql -c "GRANT ALL ON SCHEMA public TO remora;"

# Configure
cp .env.example .env
# Edit .env with your DATABASE_URL, REMORA_TEAM_TOKEN, etc.

# Build and run
cargo build --release -p remora-server
source .env && ./target/release/remora-server
```

#### Server environment variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | *required* | Postgres connection string |
| `REMORA_TEAM_TOKEN` | *required* | Shared secret for auth |
| `REMORA_BIND` | `0.0.0.0:7200` | Listen address |
| `REMORA_WORKSPACE_DIR` | `/var/lib/remora/workspaces` | Where session repos are cloned |
| `REMORA_RUN_TIMEOUT_SECS` | `600` | Max wall-clock time per Claude run |
| `REMORA_IDLE_TIMEOUT_SECS` | `1800` | Cleanup idle sessions after this |
| `REMORA_GLOBAL_DAILY_CAP` | `10000000` | Global daily token limit |
| `REMORA_CLAUDE_CMD` | `claude` | Path to Claude CLI binary |

### 2. Client (Neovim)

Build the bridge binary for your platform:

```bash
cargo build --release -p remora-bridge
```

Add to your Neovim config (lazy.nvim):

```lua
{
  "Logan-Garrett/remora",
  dependencies = { "nvim-telescope/telescope.nvim" },
  config = function()
    require("remora").setup({
      bridge = vim.fn.expand("path/to/remora-bridge"),
      url = "http://your-server:7200",
      token = "your-team-token",
      name = "yourname",
    })
    require("telescope").load_extension("remora")
  end,
}
```

#### Plugin setup options

| Option | Default | Description |
|---|---|---|
| `bridge` | `"remora-bridge"` | Path to the bridge binary |
| `url` | `"http://localhost:7200"` | Server URL |
| `token` | `""` | Team token |
| `name` | `vim.fn.hostname()` | Your display name |

### 3. Cross-compile for Raspberry Pi (or other aarch64 Linux)

```bash
rustup target add aarch64-unknown-linux-gnu
cargo install cargo-zigbuild
cargo zigbuild --release --target aarch64-unknown-linux-gnu
scp target/aarch64-unknown-linux-gnu/release/remora-server user@host:~/remora/
```

## Usage

### Keybindings (default `<leader>m` group)

| Key | Action |
|---|---|
| `<leader>mm` | Toggle Remora window (opens session picker if not connected) |
| `<leader>ms` | Browse sessions (Telescope) |
| `<leader>mc` | Command picker (Telescope) |
| `<leader>mn` | Create new session |
| `<leader>mr` | Run Claude |
| `<leader>mR` | Run Claude (full log) |
| `<leader>md` | Show diff |
| `<leader>mw` | Who's connected |
| `<leader>mi` | Session info |
| `<leader>ml` | Leave session |

### Slash commands (type in the prompt buffer)

| Command | Description |
|---|---|
| `/run` | Invoke Claude with context since last response |
| `/run-all` | Invoke Claude with the full event log |
| `/clear` | Reset context baseline |
| `/diff` | Git diff across all repos |
| `/add <path>` | Inline a file as context |
| `/fetch <url>` | Fetch and inline URL content |
| `/who` | List connected participants |
| `/session info` | Current session metadata |
| `/repo list` | List repos in session |
| `/repo add <url>` | Clone a repo into the workspace |
| `/repo remove <name>` | Remove a repo |
| `/allowlist` | Show fetch domain allowlist |
| `/allowlist add <domain>` | Pre-approve a domain |
| `/approve <domain>` | Approve a pending fetch |
| `/deny <domain>` | Deny a pending fetch |
| `/kick <name>` | Remove a participant |
| `/join <id>` | Switch to another session |
| `/sessions` | List all sessions |
| `/help` | Show command list |

### REST API

| Method | Path | Description |
|---|---|---|
| `POST` | `/sessions` | Create session `{description, repos: [url]}` |
| `GET` | `/sessions` | List sessions |
| `DELETE` | `/sessions/:id` | Delete session + cleanup |
| `GET` | `/sessions/:id` | WebSocket upgrade (query: `token`, `name`) |

All endpoints require `Authorization: Bearer <token>` header (or `token` query param for WS).
