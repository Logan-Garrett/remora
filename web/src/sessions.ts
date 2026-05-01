import { el, clear } from "./dom";
import { listSessions, createSession, deleteSession } from "./api";
import type { ConnectionConfig, SessionInfo } from "./types";

function showCreateModal(
  config: ConnectionConfig,
  onCreated: (session: SessionInfo) => void
): void {
  const reposInput = el("input", {
    type: "text",
    placeholder: "https://github.com/user/repo.git (space-separated)",
  }) as HTMLInputElement;

  const descInput = el("input", {
    type: "text",
    placeholder: "What is this session for?",
  }) as HTMLInputElement;

  const errorEl = el("div", { class: "login-error" });
  const createBtn = el("button", { class: "primary" }, "Create & Join");
  const cancelBtn = el("button", {}, "Cancel");

  const modal = el(
    "div",
    { class: "modal" },
    el("h2", {}, "New Session"),
    el("div", { class: "field" }, el("label", {}, "Git Repos (optional)"), reposInput),
    el("div", { class: "field" }, el("label", {}, "Description"), descInput),
    errorEl,
    el("div", { class: "modal-buttons" }, cancelBtn, createBtn)
  );

  const overlay = el("div", { class: "modal-overlay" }, modal);
  document.body.appendChild(overlay);

  cancelBtn.addEventListener("click", () => {
    document.body.removeChild(overlay);
  });

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) document.body.removeChild(overlay);
  });

  async function handleCreate(): Promise<void> {
    const desc = descInput.value.trim();
    if (!desc) {
      errorEl.textContent = "Description is required";
      return;
    }

    const reposRaw = reposInput.value.trim();
    const repos = reposRaw ? reposRaw.split(/\s+/).filter(Boolean) : undefined;

    createBtn.disabled = true;
    createBtn.textContent = "Creating...";
    errorEl.textContent = "";

    try {
      const session = await createSession(config, desc, repos);
      document.body.removeChild(overlay);
      onCreated(session);
    } catch (err) {
      errorEl.textContent = `${err}`;
      createBtn.disabled = false;
      createBtn.textContent = "Create & Join";
    }
  }

  createBtn.addEventListener("click", handleCreate);
  descInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") handleCreate();
  });

  reposInput.focus();
}

export function renderSessions(
  container: HTMLElement,
  config: ConnectionConfig,
  onJoin: (session: SessionInfo) => void,
  onDisconnect: () => void
): void {
  clear(container);

  const newBtn = el("button", { class: "primary" }, "New Session");
  newBtn.addEventListener("click", () => {
    showCreateModal(config, onJoin);
  });

  const header = el(
    "div",
    { class: "header" },
    el("span", { class: "header-title" }, "Remora"),
    el("span", { class: "header-status" }, `Connected as ${config.name}`),
    el(
      "div",
      { class: "header-actions" },
      newBtn,
      (() => {
        const btn = el("button", {}, "Disconnect");
        btn.addEventListener("click", onDisconnect);
        return btn;
      })()
    )
  );

  const listContainer = el("div", { class: "sessions-list" });

  const view = el(
    "div",
    { class: "sessions-view" },
    header,
    listContainer
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
          "No sessions yet. Click \"New Session\" to create one."
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

  loadSessions();
}
