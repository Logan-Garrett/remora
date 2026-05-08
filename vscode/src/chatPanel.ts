import * as vscode from "vscode";
import { RemoraConnection, ConnectionStatus } from "./connection";
import { parseCommand } from "./commands";
import type {
  ConnectionConfig,
  SessionInfo,
  ServerMessage,
  RemoraEvent,
  StreamDelta,
  ClientMessage,
} from "./types";

export class ChatPanelProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "remora.chat";

  private view?: vscode.WebviewView;
  private connection?: RemoraConnection;
  private config?: ConnectionConfig;
  private session?: SessionInfo;

  constructor(private readonly extensionUri: vscode.Uri) {}

  resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
    this.view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [],
    };

    webviewView.webview.html = this.getHtml();

    webviewView.webview.onDidReceiveMessage((message: { type: string; text?: string }) => {
      if (message.type === "send" && message.text) {
        this.handleUserInput(message.text);
      }
    });

    webviewView.onDidDispose(() => {
      this.connection?.disconnect();
      this.view = undefined;
    });
  }

  /** Connect to a session and start receiving events. */
  joinSession(config: ConnectionConfig, session: SessionInfo): void {
    this.config = config;
    this.session = session;

    // Disconnect any existing connection
    this.connection?.disconnect();

    this.connection = new RemoraConnection(
      config.url,
      config.token,
      session.id,
      config.name,
      (msg: ServerMessage) => this.handleServerMessage(msg),
      (status: ConnectionStatus) => this.postToWebview({ type: "status", status })
    );

    this.postToWebview({
      type: "joined",
      sessionDescription: session.description || session.id.slice(0, 8),
      selfName: config.name,
    });

    this.connection.connect();
  }

  /** Leave the current session. */
  leaveSession(): void {
    this.connection?.disconnect();
    this.connection = undefined;
    this.session = undefined;
    this.postToWebview({ type: "left" });
  }

  /** Send a raw message to the server (used by the remora.send command). */
  sendRawText(text: string): void {
    this.handleUserInput(text);
  }

  get isConnected(): boolean {
    return this.connection?.connected ?? false;
  }

  get currentSession(): SessionInfo | undefined {
    return this.session;
  }

  private handleUserInput(text: string): void {
    if (!this.config || !this.connection) {
      vscode.window.showWarningMessage("Remora: Not connected to a session");
      return;
    }

    const msg: ClientMessage | null = parseCommand(text, this.config.name);
    if (msg) {
      this.connection.send(msg);
    }
  }

  private handleServerMessage(msg: ServerMessage): void {
    switch (msg.type) {
      case "event":
        this.postToWebview({ type: "event", data: msg.data });
        break;
      case "error":
        this.postToWebview({ type: "error", message: msg.message });
        break;
      case "stream_start":
        this.postToWebview({ type: "stream_start" });
        break;
      case "stream_delta":
        this.postToWebview({ type: "stream_delta", delta: (msg as StreamDelta).delta });
        break;
      case "stream_end":
        this.postToWebview({ type: "stream_end" });
        break;
    }
  }

  private postToWebview(message: object): void {
    this.view?.webview.postMessage(message);
  }

  private getHtml(): string {
    return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
  :root {
    --bg: #1a1b26;
    --bg-surface: #24283b;
    --bg-hover: #2f3350;
    --bg-input: #1a1b26;
    --text: #c0caf5;
    --text-muted: #565f89;
    --text-dim: #737aa2;
    --accent: #7aa2f7;
    --accent-hover: #89b4fa;
    --green: #9ece6a;
    --red: #f7768e;
    --yellow: #e0af68;
    --orange: #ff9e64;
    --border: #3b4261;
    --radius: 6px;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    font-family: var(--vscode-font-family, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace);
    background: var(--vscode-sideBar-background, var(--bg));
    color: var(--vscode-sideBar-foreground, var(--text));
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    background: var(--bg-surface);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    min-height: 36px;
  }

  .header-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--accent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header-status {
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .header-status.connected {
    color: var(--green);
  }

  .header-status.connecting {
    color: var(--yellow);
  }

  .welcome {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 20px;
    text-align: center;
  }

  .welcome-text {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.6;
  }

  .welcome-text strong {
    color: var(--accent);
    font-weight: 600;
  }

  .chat-container {
    display: none;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .chat-container.active {
    display: flex;
  }

  .chat-messages {
    flex: 1;
    overflow-y: auto;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .chat-event {
    max-width: 95%;
    padding: 6px 10px;
    border-radius: var(--radius);
    font-size: 12px;
    line-height: 1.4;
    word-wrap: break-word;
    overflow-wrap: break-word;
  }

  .chat-event.chat {
    background: var(--bg-surface);
    border: 1px solid var(--border);
  }

  .chat-event.chat.self {
    align-self: flex-end;
    background: rgba(122, 162, 247, 0.12);
    border-color: rgba(122, 162, 247, 0.25);
  }

  .chat-event.system,
  .chat-event.system-presence,
  .chat-event.system-run,
  .chat-event.system-run-done,
  .chat-event.system-error {
    align-self: center;
    font-size: 11px;
    font-style: italic;
    padding: 3px 0;
  }

  .chat-event.system,
  .chat-event.system-presence {
    color: var(--text-muted);
  }

  .chat-event.system-run {
    color: var(--yellow);
  }

  .chat-event.system-run-done {
    color: var(--green);
  }

  .chat-event.system-error {
    color: var(--red);
  }

  .chat-event.tool-call {
    background: rgba(224, 175, 104, 0.06);
    border: 1px solid rgba(224, 175, 104, 0.15);
    width: 100%;
    max-width: 100%;
    font-size: 11px;
  }

  .chat-event.tool-result {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    width: 100%;
    max-width: 100%;
    font-size: 11px;
  }

  .chat-event.tool-result.tool-error {
    border-color: rgba(247, 118, 142, 0.3);
  }

  .tool-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tool-badge {
    background: rgba(224, 175, 104, 0.2) !important;
    color: var(--yellow) !important;
  }

  .result-badge {
    background: rgba(122, 162, 247, 0.15) !important;
    color: var(--accent) !important;
  }

  .error-badge {
    background: rgba(247, 118, 142, 0.15) !important;
    color: var(--red) !important;
  }

  .tool-summary {
    color: var(--text-dim);
    font-size: 11px;
  }

  .tool-detail {
    margin-top: 3px;
    font-size: 11px;
  }

  .chat-event.claude {
    background: rgba(158, 206, 106, 0.08);
    border: 1px solid rgba(158, 206, 106, 0.2);
    max-width: 98%;
  }

  .claude-author {
    color: var(--green) !important;
  }

  .chat-event.file,
  .chat-event.diff,
  .chat-event.fetch {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    width: 100%;
    max-width: 100%;
  }

  .chat-event .author {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    margin-bottom: 1px;
  }

  .chat-event .timestamp {
    font-size: 10px;
    color: var(--text-muted);
    margin-left: 6px;
    font-weight: 400;
  }

  .chat-event .content {
    white-space: pre-wrap;
  }

  .chat-event .content-code {
    white-space: pre;
    overflow-x: auto;
    font-family: var(--vscode-editor-font-family, "SF Mono", "Fira Code", "Cascadia Code", monospace);
    font-size: 11px;
    background: var(--bg);
    padding: 6px;
    border-radius: 4px;
    margin-top: 3px;
    max-height: 200px;
  }

  .chat-event .badge {
    display: inline-block;
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 3px;
    background: rgba(122, 162, 247, 0.15);
    color: var(--accent);
    margin-bottom: 3px;
  }

  .chat-event.streaming {
    opacity: 0.8;
    border-left: 2px solid var(--accent);
  }

  .chat-event.streaming .content::after {
    content: "\\2588";
    animation: blink 1s step-end infinite;
  }

  @keyframes blink {
    50% { opacity: 0; }
  }

  .error-msg {
    color: var(--red);
    font-size: 11px;
    padding: 4px 10px;
    text-align: center;
  }

  .chat-input-bar {
    display: flex;
    gap: 6px;
    padding: 8px 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-surface);
    flex-shrink: 0;
  }

  .chat-input-bar input {
    flex: 1;
    font-family: inherit;
    font-size: 12px;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-input);
    color: var(--text);
    outline: none;
  }

  .chat-input-bar input:focus {
    border-color: var(--accent);
  }

  .chat-input-bar input::placeholder {
    color: var(--text-muted);
  }

  .chat-input-bar button {
    font-family: inherit;
    font-size: 12px;
    padding: 5px 10px;
    border: none;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--bg);
    cursor: pointer;
    font-weight: 600;
    flex-shrink: 0;
  }

  .chat-input-bar button:hover {
    background: var(--accent-hover);
  }

  ::-webkit-scrollbar {
    width: 5px;
  }

  ::-webkit-scrollbar-track {
    background: transparent;
  }

  ::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 3px;
  }

  ::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }
</style>
</head>
<body>

<div class="welcome" id="welcome">
  <div class="welcome-text">
    <strong>Remora</strong><br><br>
    Use the command palette to connect:<br>
    <strong>Remora: Connect to Server</strong><br>
    <strong>Remora: Join Session</strong>
  </div>
</div>

<div class="chat-container" id="chat-container">
  <div class="header">
    <span class="header-title" id="header-title">Remora</span>
    <span class="header-status" id="header-status">Disconnected</span>
  </div>
  <div class="chat-messages" id="messages"></div>
  <div class="chat-input-bar">
    <input type="text" id="chat-input" placeholder="Type a message or /command..." />
    <button id="send-btn">Send</button>
  </div>
</div>

<script>
(function() {
  const vscode = acquireVsCodeApi();

  const welcomeEl = document.getElementById("welcome");
  const chatEl = document.getElementById("chat-container");
  const messagesEl = document.getElementById("messages");
  const headerTitle = document.getElementById("header-title");
  const headerStatus = document.getElementById("header-status");
  const chatInput = document.getElementById("chat-input");
  const sendBtn = document.getElementById("send-btn");

  let selfName = "";
  let streamingDiv = null;
  let streamBuffer = "";

  function formatTime(iso) {
    try {
      return new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    } catch {
      return "";
    }
  }

  function classifySystem(text) {
    if (text.startsWith("Failed") || text.includes("error")) return "system-error";
    if (text.includes("started a Claude run")) return "system-run";
    if (text.includes("run completed")) return "system-run-done";
    if (text.includes("joined") || text.includes("left")) return "system-presence";
    return "system";
  }

  function renderEvent(event) {
    const kind = event.kind;
    const text = event.payload.text || "";

    if (kind === "system" || kind === "clear_marker" || kind === "kick") {
      const cls = classifySystem(text);
      const div = document.createElement("div");
      div.className = "chat-event " + cls;
      div.textContent = text || kind;
      return div;
    }

    if (kind === "tool_call") {
      const tool = event.payload.tool || "unknown";
      const args = event.payload.args || {};
      const desc = args.description || "";
      const command = args.command || "";
      const filePath = args.file_path || "";

      const div = document.createElement("div");
      div.className = "chat-event tool-call";

      const header = document.createElement("div");
      header.className = "tool-header";

      const badge = document.createElement("span");
      badge.className = "badge tool-badge";
      badge.textContent = tool;
      header.appendChild(badge);

      const summary = document.createElement("span");
      summary.className = "tool-summary";
      summary.textContent = desc || command || filePath || JSON.stringify(args);
      header.appendChild(summary);

      div.appendChild(header);

      const detail = command || filePath;
      if (detail) {
        const code = document.createElement("div");
        code.className = "content-code tool-detail";
        code.textContent = detail;
        div.appendChild(code);
      }

      return div;
    }

    if (kind === "tool_result") {
      const output = event.payload.output || "";
      const isError = event.payload.is_error;

      const div = document.createElement("div");
      div.className = "chat-event tool-result" + (isError ? " tool-error" : "");

      const badge = document.createElement("span");
      badge.className = "badge " + (isError ? "error-badge" : "result-badge");
      badge.textContent = isError ? "ERROR" : "RESULT";
      div.appendChild(badge);

      if (output) {
        const code = document.createElement("div");
        code.className = "content-code";
        code.textContent = output;
        div.appendChild(code);
      }

      return div;
    }

    if (kind === "claude_response") {
      const div = document.createElement("div");
      div.className = "chat-event claude";

      const headerDiv = document.createElement("div");
      const authorSpan = document.createElement("span");
      authorSpan.className = "author claude-author";
      authorSpan.textContent = "Claude";
      headerDiv.appendChild(authorSpan);

      const timeSpan = document.createElement("span");
      timeSpan.className = "timestamp";
      timeSpan.textContent = formatTime(event.timestamp);
      headerDiv.appendChild(timeSpan);

      const content = document.createElement("div");
      content.className = "content";
      content.textContent = text;

      div.appendChild(headerDiv);
      div.appendChild(content);
      return div;
    }

    if (kind === "chat") {
      const isSelf = event.author === selfName;
      const div = document.createElement("div");
      div.className = "chat-event chat" + (isSelf ? " self" : "");

      const headerDiv = document.createElement("div");
      const authorSpan = document.createElement("span");
      authorSpan.className = "author";
      authorSpan.textContent = event.author || "unknown";
      headerDiv.appendChild(authorSpan);

      const timeSpan = document.createElement("span");
      timeSpan.className = "timestamp";
      timeSpan.textContent = formatTime(event.timestamp);
      headerDiv.appendChild(timeSpan);

      const content = document.createElement("div");
      content.className = "content";
      content.textContent = text;

      div.appendChild(headerDiv);
      div.appendChild(content);
      return div;
    }

    if (kind === "file" || kind === "diff" || kind === "fetch") {
      const div = document.createElement("div");
      div.className = "chat-event " + kind;

      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = kind.toUpperCase();

      const label = document.createElement("div");
      label.appendChild(badge);

      if (kind === "file" && event.payload.path) {
        label.appendChild(document.createTextNode(" " + event.payload.path));
      }
      if (kind === "fetch" && event.payload.url) {
        label.appendChild(document.createTextNode(" " + event.payload.url));
      }

      const codeContent = event.payload.content || text || "";
      const code = document.createElement("div");
      code.className = "content-code";
      code.textContent = codeContent;

      div.appendChild(label);
      div.appendChild(code);
      return div;
    }

    // Fallback
    const div = document.createElement("div");
    div.className = "chat-event system";
    div.textContent = "[" + kind + "] " + (text || JSON.stringify(event.payload));
    return div;
  }

  function scrollToBottom() {
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function sendMessage() {
    const text = chatInput.value.trim();
    if (!text) return;
    vscode.postMessage({ type: "send", text: text });
    chatInput.value = "";
    chatInput.focus();
  }

  sendBtn.addEventListener("click", sendMessage);
  chatInput.addEventListener("keydown", function(e) {
    if (e.key === "Enter") sendMessage();
  });

  window.addEventListener("message", function(e) {
    const msg = e.data;

    switch (msg.type) {
      case "joined":
        selfName = msg.selfName || "";
        headerTitle.textContent = msg.sessionDescription || "Remora";
        welcomeEl.style.display = "none";
        chatEl.classList.add("active");
        while (messagesEl.firstChild) messagesEl.removeChild(messagesEl.firstChild);
        chatInput.focus();
        break;

      case "left":
        welcomeEl.style.display = "flex";
        chatEl.classList.remove("active");
        while (messagesEl.firstChild) messagesEl.removeChild(messagesEl.firstChild);
        headerTitle.textContent = "Remora";
        headerStatus.textContent = "Disconnected";
        headerStatus.className = "header-status";
        selfName = "";
        break;

      case "status":
        headerStatus.textContent = msg.status.charAt(0).toUpperCase() + msg.status.slice(1);
        headerStatus.className = "header-status" + (msg.status === "connected" ? " connected" : msg.status === "connecting" ? " connecting" : "");
        break;

      case "event":
        {
          const rendered = renderEvent(msg.data);
          messagesEl.appendChild(rendered);
          scrollToBottom();
        }
        break;

      case "error":
        {
          const errDiv = document.createElement("div");
          errDiv.className = "error-msg";
          errDiv.textContent = "Error: " + msg.message;
          messagesEl.appendChild(errDiv);
          scrollToBottom();
        }
        break;

      case "stream_start":
        streamBuffer = "";
        streamingDiv = document.createElement("div");
        streamingDiv.className = "chat-event claude streaming";
        const streamContent = document.createElement("div");
        streamContent.className = "content";
        streamContent.textContent = "";
        streamingDiv.appendChild(streamContent);
        messagesEl.appendChild(streamingDiv);
        scrollToBottom();
        break;

      case "stream_delta":
        streamBuffer += msg.delta;
        if (streamingDiv) {
          const content = streamingDiv.querySelector(".content");
          if (content) {
            content.textContent = streamBuffer;
          }
          scrollToBottom();
        }
        break;

      case "stream_end":
        if (streamingDiv) {
          streamingDiv.remove();
          streamingDiv = null;
        }
        streamBuffer = "";
        break;
    }
  });
})();
</script>
</body>
</html>`;
  }
}
