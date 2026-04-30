import { el, clear } from "./dom";
import { buildWsUrl } from "./api";
import { RemoraSocket } from "./ws";
import { parseCommand } from "./commands";
import type { ConnectionConfig, SessionInfo, RemoraEvent, ClientMessage } from "./types";

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function renderEvent(event: RemoraEvent, selfName: string): HTMLElement {
  const kind = event.kind;

  // System events (joins, info, errors)
  if (kind === "system" || kind === "clear_marker" || kind === "kick") {
    const text =
      (event.payload.text as string) ?? kind;
    const div = el("div", { class: "chat-event system" });
    div.textContent = text;
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
    content.textContent = (event.payload.text as string) ?? "";

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
      (event.payload.content as string) ??
      (event.payload.text as string) ??
      "";

    const code = el("div", { class: "content-code" });
    code.textContent = codeContent;

    div.appendChild(label);
    div.appendChild(code);
    return div;
  }

  // Fallback for unknown event kinds
  const div = el("div", { class: "chat-event system" });
  const text = (event.payload.text as string) ?? JSON.stringify(event.payload);
  div.textContent = `[${kind}] ${text}`;
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

  const header = el(
    "div",
    { class: "header" },
    el(
      "span",
      { class: "header-title" },
      session.description || session.id.slice(0, 8)
    ),
    statusEl,
    el("div", { class: "header-actions" }, leaveBtn)
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
