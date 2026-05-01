---
name: remora
description: Chat with a Remora collaborative Claude session — list sessions, send messages, trigger /run, and read responses
user-invocable: true
allowed-tools:
  - Bash(*/scripts/remora-cli.sh *)
  - Bash(cat *)
---

# Remora Chat Skill

You are interacting with a **Remora** collaborative Claude Code session server. Remora lets multiple developers share a single Claude session with a shared, append-only event log.

## CLI Tool

Use `scripts/remora-cli.sh` (in the repo root) for all interactions. It wraps the REST API and WebSocket bridge.

## Available Commands

```bash
# Check if the server is up
scripts/remora-cli.sh health

# List all sessions
scripts/remora-cli.sh sessions

# Create a new session
scripts/remora-cli.sh create "session description"

# Delete a session
scripts/remora-cli.sh delete <session-id>

# Send a chat message to a session (connects, sends, reads a few events, disconnects)
scripts/remora-cli.sh send <session-id> "Claude-Agent" "your message here"

# Send a message and listen for responses (default 15 seconds)
scripts/remora-cli.sh chat <session-id> "Claude-Agent" "your message" 15

# Listen to a session's events for N seconds (read-only)
scripts/remora-cli.sh listen <session-id> "Claude-Agent" 10

# Trigger /run (invoke Claude in the session) and listen for the response
scripts/remora-cli.sh run <session-id> "Claude-Agent" 60
```

## How to Use

1. **List sessions** first to see what's available
2. **Join a session** by sending a message or listening
3. **Send context** as chat messages — other participants and Claude will see them
4. **Trigger `/run`** to have the remote Claude process all recent context
5. **Read the response** from the event stream

## Display Name

Always use `"Claude-Agent"` as the display name when connecting, so human participants know it's an automated agent.

## Event Format

Events come back as JSON lines. Each has a `type` field:
- `{"type":"event","data":{"kind":"chat","author":"alice","payload":{"text":"..."}}}` — a chat message
- `{"type":"event","data":{"kind":"assistant_response","payload":{"text":"..."}}}` — Claude's response
- `{"type":"event","data":{"kind":"tool_use","payload":{...}}}` — Claude tool call
- `{"type":"error","message":"..."}` — an error

## Important

- You are connecting as a **participant**. Other humans in the session will see your messages.
- The server at `REMORA_URL` (defaults to `https://the502.configurationproxy.com`) must be running.
- The `remora-bridge` binary must be built (`cargo build --release -p remora-bridge`).
