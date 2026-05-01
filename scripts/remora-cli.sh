#!/usr/bin/env bash
# remora-cli.sh — CLI wrapper for Remora REST + WebSocket API.
# Used by the Claude Code skill to interact with Remora sessions.
#
# Usage:
#   remora-cli.sh health
#   remora-cli.sh sessions
#   remora-cli.sh create "description"
#   remora-cli.sh delete <session-id>
#   remora-cli.sh send <session-id> <name> "message"
#   remora-cli.sh listen <session-id> <name> [seconds]
#   remora-cli.sh chat <session-id> <name> "message" [wait-seconds]

set -euo pipefail

SERVER="${REMORA_URL:-https://the502.configurationproxy.com}"
TOKEN="${REMORA_TEAM_TOKEN:-changeme}"
BRIDGE="${REMORA_BRIDGE:-$(dirname "$0")/../target/release/remora-bridge}"

cmd="${1:-help}"
shift || true

case "$cmd" in
  health)
    curl -sf "$SERVER/health"
    ;;

  sessions)
    curl -sf -H "Authorization: Bearer $TOKEN" "$SERVER/sessions"
    ;;

  create)
    DESC="${1:-Claude Code session}"
    curl -sf -X POST "$SERVER/sessions" \
      -H "Authorization: Bearer $TOKEN" \
      -H "Content-Type: application/json" \
      -d "{\"description\":\"$DESC\",\"repos\":[]}"
    ;;

  delete)
    SID="$1"
    curl -sf -o /dev/null -w "%{http_code}" -X DELETE \
      -H "Authorization: Bearer $TOKEN" "$SERVER/sessions/$SID"
    ;;

  send)
    # Send a single message and disconnect
    SID="$1"; NAME="$2"; MSG="$3"
    WS_URL="${SERVER/https:/wss:}/sessions/$SID?token=$TOKEN&name=$NAME"
    WS_URL="${WS_URL/http:/ws:}"
    echo "{\"type\":\"chat\",\"author\":\"$NAME\",\"text\":\"$MSG\"}" | \
      timeout 5 "$BRIDGE" "$WS_URL" 2>/dev/null | head -20
    ;;

  listen)
    # Connect and listen for events for N seconds
    SID="$1"; NAME="$2"; SECS="${3:-10}"
    WS_URL="${SERVER/https:/wss:}/sessions/$SID?token=$TOKEN&name=$NAME"
    WS_URL="${WS_URL/http:/ws:}"
    timeout "$SECS" "$BRIDGE" "$WS_URL" 2>/dev/null || true
    ;;

  chat)
    # Send a message, then listen for responses
    SID="$1"; NAME="$2"; MSG="$3"; SECS="${4:-15}"
    WS_URL="${SERVER/https:/wss:}/sessions/$SID?token=$TOKEN&name=$NAME"
    WS_URL="${WS_URL/http:/ws:}"
    (echo "{\"type\":\"chat\",\"author\":\"$NAME\",\"text\":\"$MSG\"}"; sleep "$SECS") | \
      timeout $((SECS + 2)) "$BRIDGE" "$WS_URL" 2>/dev/null || true
    ;;

  run)
    # Trigger /run and listen for Claude's response
    SID="$1"; NAME="$2"; SECS="${3:-60}"
    WS_URL="${SERVER/https:/wss:}/sessions/$SID?token=$TOKEN&name=$NAME"
    WS_URL="${WS_URL/http:/ws:}"
    (echo "{\"type\":\"run\",\"author\":\"$NAME\"}"; sleep "$SECS") | \
      timeout $((SECS + 2)) "$BRIDGE" "$WS_URL" 2>/dev/null || true
    ;;

  help|*)
    echo "Usage: remora-cli.sh <command> [args]"
    echo ""
    echo "Commands:"
    echo "  health                          Check server health"
    echo "  sessions                        List all sessions"
    echo "  create \"description\"            Create a new session"
    echo "  delete <session-id>             Delete a session"
    echo "  send <sid> <name> \"message\"     Send a message"
    echo "  listen <sid> <name> [seconds]   Listen for events (default 10s)"
    echo "  chat <sid> <name> \"msg\" [secs]  Send + listen (default 15s)"
    echo "  run <sid> <name> [seconds]      Trigger /run + listen (default 60s)"
    echo ""
    echo "Environment:"
    echo "  REMORA_URL          Server URL (default: https://the502.configurationproxy.com)"
    echo "  REMORA_TEAM_TOKEN   Auth token (default: changeme)"
    echo "  REMORA_BRIDGE       Path to remora-bridge binary"
    ;;
esac
