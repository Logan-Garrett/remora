# Track 4: Clients and Ecosystem

> Expand how people access Remora -- new editor plugins, a desktop app, mobile support, and protocol improvements. This track has the fewest server-side dependencies and can progress independently.

---

## Current State

- **Web client**: TypeScript/Vite SPA, fully responsive (mobile-tested on 6 device viewports), server-agnostic login
- **Neovim plugin**: Lua with Telescope integration, communicates via `remora-bridge` (Rust stdio-to-WebSocket binary)
- **MCP server**: TypeScript MCP server for Claude Desktop / Claude Code / Cursor / any MCP client
- **Bridge binary**: Minimal Rust binary that translates JSON on stdin/stdout to WebSocket messages
- Claude responses arrive per-turn (one chunk per agentic step), not token-by-token
- No desktop app, no VS Code extension, no JetBrains plugin

---

## Milestone 1: Token-Level Streaming

**Priority: Highest** | **Depends on: server-side Claude CLI output parsing changes**

Currently Claude's output arrives as complete response turns. Token-level streaming would show Claude "typing" in real-time, making the experience much more interactive.

### Implementation plan

1. **Server changes** (`claude.rs`): Parse Claude CLI's `--output-format stream-json` output at a finer granularity. Currently the server buffers until a complete response/tool_call/tool_result is formed. Instead, emit partial events:
   ```
   New event kinds:
   - claude_stream_start    -- Claude began generating
   - claude_stream_delta    -- partial text chunk
   - claude_stream_end      -- generation complete (followed by existing claude_response)
   ```
2. **Protocol extension** (`common/src/lib.rs`): Add new `ServerMsg` variants for streaming deltas. These are ephemeral -- they are broadcast to WebSocket subscribers but NOT persisted to the events table (only the final `claude_response` is persisted).
3. **Web client** (`chat.ts`): Accumulate deltas in a "typing" bubble that updates in real-time. When `claude_stream_end` arrives, replace with the final rendered response.
4. **Neovim plugin** (`init.lua`): Append delta text to the chat buffer in real-time.
5. **MCP server** (`index.ts`): Buffer deltas internally; the `remora_events` tool returns completed events only.

### Acceptance criteria

- Claude's text appears character-by-character (or chunk-by-chunk) in the web client and Neovim
- Only the final `claude_response` event is persisted in the database
- Clients that don't support streaming (MCP, reconnecting clients) see the same final result
- No performance regression -- deltas are fire-and-forget, no DB writes

---

## Milestone 2: VS Code Extension

**Priority: High** | **Depends on: nothing**

Bring the Neovim plugin experience to VS Code. VS Code has the largest editor market share.

### Architecture

Two options:
- **Option A (Bridge-based)**: Reuse `remora-bridge` as a subprocess, communicate via stdin/stdout. Same architecture as the Neovim plugin. Simpler to build, reuses existing code.
- **Option B (Native WebSocket)**: Use VS Code's built-in WebSocket support (via `ws` npm package). No external binary needed. Better for distribution via the VS Code marketplace.

Recommend **Option B** for distribution simplicity.

### Feature set (matching Neovim plugin)

1. **Session picker**: Command palette command to list/join sessions (equivalent to Telescope picker)
2. **Chat panel**: WebView-based panel showing the event log with syntax highlighting
3. **Input bar**: Text input at the bottom of the chat panel for messages and slash commands
4. **Slash commands**: Full parity with web client (`/run`, `/add`, `/fetch`, `/diff`, `/who`, `/trust`, etc.)
5. **Status bar**: Show connected session name and participant count
6. **Notifications**: VS Code notification when Claude finishes a run
7. **Settings**: Extension settings for server URL, token, display name

### Implementation plan

1. Scaffold the extension: `yo code` generator, TypeScript, WebView API.
2. WebSocket connection management: connect, reconnect (3 retries), disconnect.
3. Chat panel rendering: reuse the rendering logic concepts from the web client but adapted for VS Code WebView.
4. Slash command parsing: port from `web/src/commands.ts`.
5. Publish to VS Code marketplace under the `remora` publisher.

### Acceptance criteria

- Can browse sessions, create new ones, and join existing ones
- Chat panel shows events in real-time with proper formatting
- All slash commands work
- Reconnects automatically on connection drop (up to 3 times)
- Available on the VS Code marketplace

---

## Milestone 3: Desktop App (Tauri)

**Priority: Medium** | **Depends on: nothing (wraps the existing web client)**

A native desktop app that wraps the web client with platform-native features.

### Benefits over browser

- Menu bar / system tray presence
- Native notifications (Claude finished, someone joined, you were mentioned)
- Deep links: `remora://join/<session-id>` opens the app and joins a session
- No browser tab management
- Auto-start on login (optional)

### Implementation plan

1. Scaffold with `create-tauri-app`. The frontend is the existing `web/dist/` build.
2. **System tray**: Show connected session count, quick-join menu for recent sessions.
3. **Notifications**: Use Tauri's notification API. Trigger on `claude_response` events and `system` join events.
4. **Deep links**: Register `remora://` protocol handler. Parse `remora://join/<url>/<session-id>/<token>`.
5. **Auto-update**: Use Tauri's built-in updater pointed at GitHub Releases.
6. **Build targets**: macOS (universal), Windows (x64), Linux (AppImage + deb).
7. Add to the `release.yml` workflow: build and attach desktop app binaries to releases.

### Acceptance criteria

- App launches, shows login, connects to a server
- System tray icon with session status
- Native notifications work on macOS and Windows
- Deep links open the app and join the correct session
- Auto-update checks GitHub Releases on startup

---

## Milestone 4: JetBrains Plugin

**Priority: Medium-Low** | **Depends on: nothing**

Same feature set as VS Code, targeting IntelliJ IDEA, WebStorm, PyCharm, and other JetBrains IDEs.

### Architecture

JetBrains plugins are written in Kotlin (or Java). WebSocket support is available via `ktor-client` or `okhttp`. No bridge binary needed.

### Feature set

- Tool window with session picker and chat panel
- Slash command input
- Status bar widget showing session info
- Notifications via IDE notification system

### Implementation plan

1. Scaffold with IntelliJ Platform Plugin Template (Kotlin).
2. WebSocket client using `ktor-client-websockets`.
3. Tool window with JList for session picker and JEditorPane/JBTextArea for chat.
4. Slash command parsing (port from TypeScript).
5. Publish to JetBrains Marketplace.

### Acceptance criteria

- Works in IntelliJ IDEA 2024.1+ (and compatible IDEs)
- Feature parity with VS Code extension
- Published on JetBrains Marketplace

---

## Milestone 5: Mobile App / PWA

**Priority: Low** | **Depends on: nothing**

A dedicated mobile experience beyond the responsive web client.

### Options

- **PWA (recommended first)**: The web client is already responsive. Adding a `manifest.json`, service worker, and install prompt turns it into an installable PWA with offline caching and push notifications.
- **React Native / Expo**: Full native app with push notifications. Higher effort, more capabilities.

Recommend PWA first, native app later if demand justifies it.

### PWA implementation plan

1. Add `web/public/manifest.json` with app name, icons, theme color.
2. Add a service worker for offline caching of the SPA shell.
3. Add push notification support using the Push API. Server sends push notifications for `claude_response` and `system` events when the app is backgrounded.
4. Add "install app" prompt in the login screen.
5. Server-side: new endpoint `POST /push/subscribe` to register push subscriptions (requires `web-push` crate or service).

### Acceptance criteria

- Web client is installable as a PWA on iOS and Android
- Push notifications fire when Claude finishes a run (app backgrounded)
- Offline: the app shell loads without network, shows a "reconnecting" state

---

## Milestone 6: CLI Client

**Priority: Low** | **Depends on: nothing**

A standalone command-line client for interacting with Remora sessions without a GUI. The `remora-cli.sh` script exists but is limited.

### Implementation plan

1. Extend `remora-bridge` (or create a new `remora-cli` binary) with a REPL mode:
   ```
   remora-cli connect <url> <session-id> <token> [--name <name>]
   ```
2. The REPL shows events as they arrive (formatted for terminal) and accepts slash commands as input.
3. Support `--non-interactive` mode for scripting: pipe messages in, get events out as JSON lines.
4. Useful for CI/CD pipelines, headless servers, and SSH-only environments.

### Acceptance criteria

- Interactive REPL connects and shows events
- All slash commands work
- Non-interactive mode supports piped input/output
- Works over SSH without a GUI

---

## Dependency Graph

```
M1 (Token-level streaming) -- server changes benefit all clients
M2 (VS Code extension)     -- independent
M3 (Desktop app)           -- independent (wraps web client)
M4 (JetBrains plugin)      -- independent
M5 (Mobile PWA)            -- independent (extends web client)
M6 (CLI client)            -- independent (extends bridge binary)
```

All milestones are independent of each other. M1 (streaming) is listed first because it improves the experience across all existing and future clients.

---

## Risks and Open Questions

- **Token-level streaming**: The Claude CLI's `stream-json` format may not expose individual tokens -- it may only emit chunks. The delta granularity depends on what the CLI provides. Test with the current CLI version before committing to a specific delta format.
- **VS Code WebView**: WebViews in VS Code have restrictions (no `eval`, CSP headers). The chat panel rendering needs to work within these constraints. Consider using a framework like Lit or Preact that compiles to plain DOM manipulation.
- **Tauri bundle size**: Tauri apps include a WebView runtime on Windows (WebView2) and Linux (webkit2gtk). macOS uses WKWebView natively. Total bundle size is typically 5-15 MB, much smaller than Electron.
- **JetBrains compatibility**: JetBrains IDEs have strict plugin compatibility requirements. The plugin must target a specific `platformVersion` range and be tested against multiple IDE versions.
- **Push notifications**: Web Push requires VAPID keys and a push service. This adds server-side complexity. Consider whether the existing WebSocket connection (when the app is open) is sufficient for most users.
- **CLI vs. bridge**: The existing `remora-bridge` is designed for Neovim (stdin/stdout JSON protocol). A CLI client needs a different UX (human-readable output, line-based input). Either extend the bridge with a `--mode cli` flag or create a separate binary.
