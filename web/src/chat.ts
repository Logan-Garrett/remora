import { el, clear } from "./dom";
import { buildWsUrl, getOwnerKey, storeOwnerKey } from "./api";
import { RemoraSocket } from "./ws";
import { parseCommand } from "./commands";
import type { ConnectionConfig, SessionInfo, RemoraEvent, ClientMessage } from "./types";

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function classifySystem(text: string): string {
  if (text.startsWith("Failed") || text.includes("error")) return "system-error";
  if (text.includes("started a Claude run")) return "system-run";
  if (text.includes("run completed")) return "system-run-done";
  if (text.includes("joined") || text.includes("left")) return "system-presence";
  return "system";
}

function renderEvent(event: RemoraEvent, selfName: string): HTMLElement {
  const kind = event.kind;
  const text = (event.payload.text as string) ?? "";

  // System events (joins, info, errors, run status)
  if (kind === "system" || kind === "clear_marker" || kind === "kick") {
    const cls = classifySystem(text);
    const div = el("div", { class: `chat-event ${cls}` });
    div.textContent = text || kind;
    return div;
  }

  // Tool calls — show tool name + description
  if (kind === "tool_call") {
    const tool = (event.payload.tool as string) ?? "unknown";
    const args = event.payload.args as Record<string, unknown> | undefined;
    const desc = (args?.description as string) ?? "";
    const command = (args?.command as string) ?? "";
    const filePath = (args?.file_path as string) ?? "";

    const div = el("div", { class: "chat-event tool-call" });

    const badge = el("span", { class: "badge tool-badge" });
    badge.textContent = tool;

    const summary = el("span", { class: "tool-summary" });
    summary.textContent = desc || command || filePath || JSON.stringify(args);

    const header = el("div", { class: "tool-header" }, badge, summary);
    div.appendChild(header);

    // Show command/path in a code block if present
    const detail = command || filePath;
    if (detail) {
      const code = el("div", { class: "content-code tool-detail" });
      code.textContent = detail;
      div.appendChild(code);
    }

    return div;
  }

  // Tool results — show output
  if (kind === "tool_result") {
    const output = (event.payload.output as string) ?? "";
    const isError = event.payload.is_error as boolean;

    const div = el("div", { class: `chat-event tool-result${isError ? " tool-error" : ""}` });

    const badge = el("span", { class: `badge ${isError ? "error-badge" : "result-badge"}` });
    badge.textContent = isError ? "ERROR" : "RESULT";

    div.appendChild(badge);

    if (output) {
      const code = el("div", { class: "content-code" });
      code.textContent = output;
      div.appendChild(code);
    }

    return div;
  }

  // Claude responses
  if (kind === "claude_response") {
    const div = el("div", { class: "chat-event claude" });

    const authorSpan = el("span", { class: "author claude-author" });
    authorSpan.textContent = "Claude";

    const timeSpan = el("span", { class: "timestamp" });
    timeSpan.textContent = formatTime(event.timestamp);

    const header = el("div", {}, authorSpan, timeSpan);

    const content = el("div", { class: "content" });
    content.textContent = text;

    div.appendChild(header);
    div.appendChild(content);
    return div;
  }

  // Chat messages
  if (kind === "chat") {
    const isSelf = event.author === selfName;
    const div = el("div", {
      class: `chat-event chat${isSelf ? " self" : ""}`,
    });

    const authorSpan = el("span", { class: "author" });
    authorSpan.textContent = event.author ?? "unknown";

    const timeSpan = el("span", { class: "timestamp" });
    timeSpan.textContent = formatTime(event.timestamp);

    const header = el("div", {}, authorSpan, timeSpan);

    const content = el("div", { class: "content" });
    content.textContent = text;

    div.appendChild(header);
    div.appendChild(content);
    return div;
  }

  // File / diff / fetch — show as code block
  if (kind === "file" || kind === "diff" || kind === "fetch") {
    const div = el("div", { class: `chat-event ${kind}` });

    const badge = el("span", { class: "badge" });
    badge.textContent = kind.toUpperCase();

    const label = el("div", {}, badge);

    if (kind === "file" && event.payload.path) {
      const pathSpan = document.createTextNode(
        ` ${event.payload.path as string}`
      );
      label.appendChild(pathSpan);
    }
    if (kind === "fetch" && event.payload.url) {
      const urlSpan = document.createTextNode(
        ` ${event.payload.url as string}`
      );
      label.appendChild(urlSpan);
    }

    const codeContent =
      (event.payload.content as string) ?? text ?? "";

    const code = el("div", { class: "content-code" });
    code.textContent = codeContent;

    div.appendChild(label);
    div.appendChild(code);
    return div;
  }

  // Fallback for unknown event kinds
  const div = el("div", { class: "chat-event system" });
  const fallback = text || JSON.stringify(event.payload);
  div.textContent = `[${kind}] ${fallback}`;
  return div;
}

export function renderChat(
  container: HTMLElement,
  config: ConnectionConfig,
  session: SessionInfo,
  onLeave: () => void
): void {
  clear(container);

  const statusEl = el("span", { class: "header-status" }, "Connecting...");
  const leaveBtn = el("button", {}, "Leave");
  leaveBtn.addEventListener("click", () => {
    socket.close();
    onLeave();
  });

  // Build header actions
  const actionsEl = el("div", { class: "header-actions" });
  const ownerKey = getOwnerKey(session.id);
  if (ownerKey) {
    // We have the key — show a button to copy it
    const keyBtn = el("button", {}, "Owner Key");
    keyBtn.addEventListener("click", () => {
      navigator.clipboard.writeText(ownerKey).then(
        () => { keyBtn.textContent = "Copied!"; setTimeout(() => { keyBtn.textContent = "Owner Key"; }, 2000); },
        () => { window.prompt("Copy your owner key:", ownerKey); }
      );
    });
    actionsEl.appendChild(keyBtn);
  } else {
    // No key stored — show a button to enter one (for rejoining after refresh)
    const enterKeyBtn = el("button", {}, "Enter Owner Key");
    enterKeyBtn.addEventListener("click", () => {
      const key = window.prompt("Paste your owner key to claim session ownership:");
      if (key && key.trim()) {
        storeOwnerKey(session.id, key.trim());
        // Reconnect with the key
        socket.close();
        onLeave();
      }
    });
    actionsEl.appendChild(enterKeyBtn);
  }
  actionsEl.appendChild(leaveBtn);

  const header = el(
    "div",
    { class: "header" },
    el(
      "span",
      { class: "header-title" },
      session.description || session.id.slice(0, 8)
    ),
    statusEl,
    actionsEl
  );

  const messagesEl = el("div", { class: "chat-messages" });
  const chatInput = el("input", {
    type: "text",
    placeholder: "Type a message or /command...",
  }) as HTMLInputElement;
  const sendBtn = el("button", { class: "primary" }, "Send");

  const inputBar = el(
    "div",
    { class: "chat-input-bar" },
    chatInput,
    sendBtn
  );

  const view = el("div", { class: "chat-view" }, header, messagesEl, inputBar);
  container.appendChild(view);

  const wsUrl = buildWsUrl(config, session.id);
  const socket = new RemoraSocket(wsUrl, {
    onEvent(event: RemoraEvent) {
      const el = renderEvent(event, config.name);
      messagesEl.appendChild(el);
      messagesEl.scrollTop = messagesEl.scrollHeight;
    },
    onError(message: string) {
      const errEl = el("div", { class: "chat-event system" });
      errEl.textContent = `Error: ${message}`;
      messagesEl.appendChild(errEl);
      messagesEl.scrollTop = messagesEl.scrollHeight;
    },
    onClose() {
      statusEl.textContent = "Disconnected";
      statusEl.className = "header-status";
    },
  });

  socket.connect();
  statusEl.textContent = "Connected";
  statusEl.className = "header-status connected";

  function sendMessage(): void {
    const text = chatInput.value.trim();
    if (!text) return;

    const msg: ClientMessage | null = parseCommand(text, config.name);
    if (msg) {
      socket.send(msg);
    }

    chatInput.value = "";
    chatInput.focus();
  }

  sendBtn.addEventListener("click", sendMessage);
  chatInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") sendMessage();
  });

  chatInput.focus();
}
