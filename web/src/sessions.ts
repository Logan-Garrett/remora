import { el, clear } from "./dom";
import {
  listSessions,
  createSession,
  deleteSession,
  reactivateSession,
  storeOwnerKey,
  getUserDashboard,
} from "./api";
import type { ConnectionConfig, SessionInfo, DashboardSession } from "./types";

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
    placeholder: "Session description",
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
      // Store the owner_key so we auto-claim ownership on WS connect
      if (session.owner_key) {
        storeOwnerKey(session.id, session.owner_key);
      }
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

/** Render a session card from either SessionInfo or DashboardSession data. */
function renderSessionCard(
  session: { id: string; description: string; created_at: string; status: string; team_name?: string | null },
  onJoin: (session: SessionInfo) => void,
  onDelete: () => void,
  config: ConnectionConfig
): HTMLElement {
  const isExpired = session.status === "expired";

  const descChildren: (HTMLElement | string)[] = [
    session.description || "(no description)",
  ];
  if (isExpired) {
    descChildren.push(
      el("span", { class: "session-badge expired" }, "expired")
    );
  }
  if (session.team_name) {
    descChildren.push(
      el("span", { class: "session-badge team" }, session.team_name)
    );
  }

  const card = el(
    "div",
    { class: isExpired ? "session-card session-expired" : "session-card" },
    el(
      "div",
      {},
      el(
        "div",
        { class: "session-desc" },
        ...descChildren
      ),
      el(
        "div",
        { class: "session-meta" },
        `ID: ${session.id.slice(0, 8)}... | Created: ${new Date(session.created_at).toLocaleString()}`
      )
    ),
    (() => {
      const actions = el("div", { class: "session-actions" });
      if (isExpired) {
        const resumeBtn = el("button", { class: "primary" }, "Resume");
        resumeBtn.addEventListener("click", async (e) => {
          e.stopPropagation();
          try {
            await reactivateSession(config, session.id);
            const sessionInfo: SessionInfo = {
              id: session.id,
              description: session.description,
              created_at: session.created_at,
              status: "active",
            };
            onJoin(sessionInfo);
          } catch (err) {
            alert(`Resume failed: ${err}`);
          }
        });
        actions.appendChild(resumeBtn);
      }
      const delBtn = el("button", { class: "danger" }, "Delete");
      delBtn.addEventListener("click", async (e) => {
        e.stopPropagation();
        if (!confirm("Delete this session?")) return;
        try {
          await deleteSession(config, session.id);
          onDelete();
        } catch (err) {
          alert(`Delete failed: ${err}`);
        }
      });
      actions.appendChild(delBtn);
      return actions;
    })()
  );

  if (!isExpired) {
    card.addEventListener("click", () => {
      const sessionInfo: SessionInfo = {
        id: session.id,
        description: session.description,
        created_at: session.created_at,
        status: session.status,
      };
      onJoin(sessionInfo);
    });
  }

  return card;
}

export function renderSessions(
  container: HTMLElement,
  config: ConnectionConfig,
  onJoin: (session: SessionInfo) => void,
  onDisconnect: () => void,
  onAdmin?: () => void,
  onTeams?: () => void
): void {
  clear(container);

  const newBtn = el("button", { class: "primary" }, "New Session");
  newBtn.addEventListener("click", () => {
    showCreateModal(config, onJoin);
  });

  const actionsDiv = el("div", { class: "header-actions" }, newBtn);

  // Teams button — visible to all authenticated users
  if (onTeams) {
    const teamsBtn = el("button", { class: "teams-btn" }, "Teams");
    teamsBtn.addEventListener("click", onTeams);
    actionsDiv.appendChild(teamsBtn);
  }

  if (config.isAdmin && onAdmin) {
    const adminBtn = el("button", { class: "admin-btn" }, "Admin");
    adminBtn.addEventListener("click", onAdmin);
    actionsDiv.appendChild(adminBtn);
  }

  const disconnectBtn = el("button", {}, "Disconnect");
  disconnectBtn.addEventListener("click", onDisconnect);
  actionsDiv.appendChild(disconnectBtn);

  const header = el(
    "div",
    { class: "header" },
    el("span", { class: "header-title" }, "Remora"),
    el("span", { class: "header-status" }, `Connected as ${config.name}`),
    actionsDiv
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

    // For JWT users (non-admin-token), try the dashboard endpoint first
    // which shows only their sessions with team annotations.
    // Team token users get all sessions.
    if (!config.isAdmin) {
      try {
        const dashboard = await getUserDashboard(config);
        renderDashboardSessions(dashboard.sessions);
        return;
      } catch {
        // Dashboard endpoint might fail for team-token users or older servers;
        // fall back to regular session list
      }
    }

    // Fallback: regular session list (all sessions)
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
      listContainer.appendChild(
        renderSessionCard(session, onJoin, loadSessions, config)
      );
    }
  }

  function renderDashboardSessions(sessions: DashboardSession[]): void {
    if (sessions.length === 0) {
      listContainer.appendChild(
        el(
          "div",
          { class: "sessions-empty" },
          "No sessions yet. Click \"New Session\" to create one, or join a team."
        )
      );
      return;
    }

    for (const session of sessions) {
      listContainer.appendChild(
        renderSessionCard(session, onJoin, loadSessions, config)
      );
    }
  }

  loadSessions();
}
