import { el, clear } from "./dom";
import { listSessions, createSession, deleteSession } from "./api";
import type { ConnectionConfig, SessionInfo } from "./types";

export function renderSessions(
  container: HTMLElement,
  config: ConnectionConfig,
  onJoin: (session: SessionInfo) => void,
  onDisconnect: () => void
): void {
  clear(container);

  const header = el(
    "div",
    { class: "header" },
    el("span", { class: "header-title" }, "Remora"),
    el("span", { class: "header-status" }, `Connected as ${config.name}`),
    el(
      "div",
      { class: "header-actions" },
      (() => {
        const btn = el("button", {}, "Disconnect");
        btn.addEventListener("click", onDisconnect);
        return btn;
      })()
    )
  );

  const listContainer = el("div", { class: "sessions-list" });
  const descInput = el("input", {
    type: "text",
    placeholder: "New session description...",
  }) as HTMLInputElement;

  const createBtn = el("button", { class: "primary" }, "Create");

  const toolbar = el(
    "div",
    { class: "sessions-toolbar" },
    descInput,
    createBtn
  );

  const view = el(
    "div",
    { class: "sessions-view" },
    header,
    listContainer,
    toolbar
  );

  container.appendChild(view);

  async function loadSessions(): Promise<void> {
    clear(listContainer);
    let sessions: SessionInfo[];
    try {
      sessions = await listSessions(config);
    } catch (err) {
      listContainer.appendChild(
        el("div", { class: "sessions-empty" }, `Error: ${err}`)
      );
      return;
    }

    if (sessions.length === 0) {
      listContainer.appendChild(
        el(
          "div",
          { class: "sessions-empty" },
          "No sessions yet. Create one below."
        )
      );
      return;
    }

    for (const session of sessions) {
      const card = el(
        "div",
        { class: "session-card" },
        el(
          "div",
          {},
          el(
            "div",
            { class: "session-desc" },
            session.description || "(no description)"
          ),
          el(
            "div",
            { class: "session-meta" },
            `ID: ${session.id.slice(0, 8)}... | Created: ${new Date(session.created_at).toLocaleString()}`
          )
        ),
        (() => {
          const actions = el("div", { class: "session-actions" });
          const delBtn = el("button", { class: "danger" }, "Delete");
          delBtn.addEventListener("click", async (e) => {
            e.stopPropagation();
            if (!confirm("Delete this session?")) return;
            try {
              await deleteSession(config, session.id);
              loadSessions();
            } catch (err) {
              alert(`Delete failed: ${err}`);
            }
          });
          actions.appendChild(delBtn);
          return actions;
        })()
      );

      card.addEventListener("click", () => onJoin(session));
      listContainer.appendChild(card);
    }
  }

  createBtn.addEventListener("click", async () => {
    const desc = descInput.value.trim();
    if (!desc) return;
    createBtn.disabled = true;
    try {
      await createSession(config, desc);
      descInput.value = "";
      loadSessions();
    } catch (err) {
      alert(`Create failed: ${err}`);
    } finally {
      createBtn.disabled = false;
    }
  });

  descInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") createBtn.click();
  });

  loadSessions();
}
