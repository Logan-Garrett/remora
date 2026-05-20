import { el, clear } from "./dom";
import {
  listTeams,
  createTeam,
  getTeam,
  updateTeam,
  deleteTeam,
  listTeamMembers,
  addTeamMember,
  updateTeamMember,
  removeTeamMember,
  listTeamSessions,
  createTeamSession,
  storeOwnerKey,
} from "./api";
import type { ConnectionConfig, SessionInfo, Team, TeamMember } from "./types";

type TeamsTab = "my-teams" | "create";

export function renderTeams(
  container: HTMLElement,
  config: ConnectionConfig,
  onBack: () => void,
  onJoinSession?: (session: SessionInfo) => void
): void {
  clear(container);

  let activeTab: TeamsTab = "my-teams";

  // ── Header ──

  const backBtn = el("button", {}, "Back to Sessions");
  backBtn.addEventListener("click", onBack);

  const header = el(
    "div",
    { class: "header" },
    el("span", { class: "header-title" }, "Teams"),
    el("div", { class: "header-actions" }, backBtn)
  );

  // ── Tabs ──

  const myTeamsTab = el("button", { class: "tab active" }, "My Teams");
  const createTab = el("button", { class: "tab" }, "Create Team");
  const allTabs = [myTeamsTab, createTab];
  const tabBar = el("div", { class: "admin-tabs" }, ...allTabs);

  const content = el("div", { class: "admin-content" });

  function switchTab(tab: TeamsTab): void {
    activeTab = tab;
    allTabs.forEach((t) => t.classList.remove("active"));
    const tabMap: Record<TeamsTab, HTMLElement> = {
      "my-teams": myTeamsTab,
      create: createTab,
    };
    tabMap[tab].classList.add("active");
    renderActiveTab();
  }

  myTeamsTab.addEventListener("click", () => switchTab("my-teams"));
  createTab.addEventListener("click", () => switchTab("create"));

  // ── Tab renderers ──

  function renderActiveTab(): void {
    clear(content);
    content.appendChild(el("div", { class: "admin-loading" }, "Loading..."));
    if (activeTab === "my-teams") renderMyTeamsTab();
    else renderCreateTab();
  }

  async function renderMyTeamsTab(): Promise<void> {
    try {
      const teams = await listTeams(config);
      clear(content);

      if (teams.length === 0) {
        content.appendChild(
          el(
            "div",
            { class: "admin-empty" },
            "No teams yet. Create one or ask a team admin to add you."
          )
        );
        return;
      }

      const list = el("div", { class: "teams-list" });
      for (const team of teams) {
        const card = el(
          "div",
          { class: "team-card" },
          el("div", { class: "team-card-info" },
            el("div", { class: "team-card-name" }, team.name),
            el("div", { class: "team-card-desc" }, team.description || "(no description)")
          ),
          el("div", { class: "team-card-meta" },
            el("span", { class: "team-card-date" },
              `Created: ${new Date(team.created_at).toLocaleDateString()}`
            )
          )
        );
        card.addEventListener("click", () => renderTeamDetail(team.id));
        list.appendChild(card);
      }
      content.appendChild(list);
    } catch (e) {
      clear(content);
      content.appendChild(el("div", { class: "admin-error" }, `Failed to load teams: ${e}`));
    }
  }

  function renderCreateTab(): void {
    clear(content);

    const nameInput = el("input", {
      type: "text",
      placeholder: "Team name",
    }) as HTMLInputElement;

    const descInput = el("input", {
      type: "text",
      placeholder: "Team description (optional)",
    }) as HTMLInputElement;

    const errorEl = el("div", { class: "login-error" });
    const createBtn = el("button", { class: "primary" }, "Create Team");

    createBtn.addEventListener("click", async () => {
      const name = nameInput.value.trim();
      if (!name) {
        errorEl.textContent = "Team name is required";
        return;
      }
      const desc = descInput.value.trim();
      createBtn.disabled = true;
      createBtn.textContent = "Creating...";
      errorEl.textContent = "";

      try {
        const team = await createTeam(config, name, desc);
        switchTab("my-teams");
        // Jump to the team detail after a brief load
        setTimeout(() => renderTeamDetail(team.id), 100);
      } catch (err) {
        errorEl.textContent = `${err}`;
        createBtn.disabled = false;
        createBtn.textContent = "Create Team";
      }
    });

    nameInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") createBtn.click();
    });

    const form = el(
      "div",
      { class: "teams-create-form" },
      el("h3", { class: "admin-section-title" }, "Create a New Team"),
      el("div", { class: "field" }, el("label", {}, "Name"), nameInput),
      el("div", { class: "field" }, el("label", {}, "Description"), descInput),
      errorEl,
      createBtn
    );

    content.appendChild(form);
    nameInput.focus();
  }

  // ── Team detail view ──

  async function renderTeamDetail(teamId: string): Promise<void> {
    clear(content);
    content.appendChild(el("div", { class: "admin-loading" }, "Loading team..."));

    try {
      const [team, members, sessions] = await Promise.all([
        getTeam(config, teamId),
        listTeamMembers(config, teamId),
        listTeamSessions(config, teamId),
      ]);
      clear(content);

      // Determine if current user is admin of this team
      const currentUserMember = members.find(
        (m) => m.display_name === config.name || m.email === config.name
      );
      const isTeamAdmin = currentUserMember?.role === "admin";

      // ── Back to list ──
      const backToListBtn = el("button", { class: "small" }, "Back to Teams");
      backToListBtn.addEventListener("click", () => renderMyTeamsTab());

      // ── Team header ──
      const teamHeader = el(
        "div",
        { class: "team-detail-header" },
        backToListBtn,
        el("h3", { class: "admin-section-title" }, team.name),
        el("div", { class: "admin-section" }, team.description || "(no description)")
      );

      // ── Edit team (admin only) ──
      if (isTeamAdmin) {
        const editBtn = el("button", { class: "small" }, "Edit");
        editBtn.addEventListener("click", () => showEditTeamModal(team, teamId));

        const deleteBtn = el("button", { class: "small danger" }, "Delete Team");
        deleteBtn.addEventListener("click", async () => {
          if (!confirm(`Delete team "${team.name}"? This cannot be undone.`)) return;
          try {
            await deleteTeam(config, teamId);
            switchTab("my-teams");
          } catch (err) {
            alert(`Delete failed: ${err}`);
          }
        });

        const adminActions = el("div", { class: "team-admin-actions" }, editBtn, deleteBtn);
        teamHeader.appendChild(adminActions);
      }

      content.appendChild(teamHeader);

      // ── Members section ──
      content.appendChild(el("h3", { class: "admin-section-title" }, `Members (${members.length})`));
      renderMembersTable(members, teamId, isTeamAdmin);

      // ── Add member form (admin only) ──
      if (isTeamAdmin) {
        renderAddMemberForm(teamId);
      }

      // ── Sessions section ──
      content.appendChild(el("h3", { class: "admin-section-title" }, `Team Sessions (${sessions.length})`));
      renderTeamSessionsList(sessions, teamId, isTeamAdmin);

    } catch (e) {
      clear(content);
      content.appendChild(el("div", { class: "admin-error" }, `Failed to load team: ${e}`));
    }
  }

  function renderMembersTable(members: TeamMember[], teamId: string, isTeamAdmin: boolean): void {
    if (members.length === 0) {
      content.appendChild(el("div", { class: "admin-empty" }, "No members"));
      return;
    }

    const rows = members.map((m) => {
      const roleCell = el("td", {});

      if (isTeamAdmin) {
        const select = el("select", { class: "role-select" }) as HTMLSelectElement;
        for (const role of ["admin", "member", "viewer"]) {
          const opt = el("option", { value: role }, role) as HTMLOptionElement;
          if (role === m.role) opt.selected = true;
          select.appendChild(opt);
        }
        select.addEventListener("change", async () => {
          if (!confirm(`Change ${m.display_name}'s role to "${select.value}"?`)) {
            select.value = m.role;
            return;
          }
          try {
            await updateTeamMember(config, teamId, m.user_id, select.value);
            m.role = select.value;
          } catch (err) {
            alert(`Error: ${err}`);
            select.value = m.role;
          }
        });
        roleCell.appendChild(select);
      } else {
        roleCell.appendChild(document.createTextNode(m.role));
      }

      const actionsCell = el("td", {});
      if (isTeamAdmin) {
        const removeBtn = el("button", { class: "small danger" }, "Remove");
        removeBtn.addEventListener("click", async () => {
          if (!confirm(`Remove ${m.display_name} from the team?`)) return;
          try {
            await removeTeamMember(config, teamId, m.user_id);
            renderTeamDetail(teamId);
          } catch (err) {
            alert(`Error: ${err}`);
          }
        });
        actionsCell.appendChild(removeBtn);
      }

      return el(
        "tr",
        {},
        el("td", {}, m.display_name),
        el("td", {}, m.email),
        roleCell,
        el("td", {}, new Date(m.joined_at).toLocaleDateString()),
        actionsCell
      );
    });

    const table = el(
      "div",
      { class: "admin-table-wrap" },
      el(
        "table",
        { class: "admin-table" },
        el(
          "thead",
          {},
          el("tr", {},
            el("th", {}, "Name"),
            el("th", {}, "Email"),
            el("th", {}, "Role"),
            el("th", {}, "Joined"),
            el("th", {}, "Actions")
          )
        ),
        el("tbody", {}, ...rows)
      )
    );

    content.appendChild(table);
  }

  function renderAddMemberForm(teamId: string): void {
    const userIdInput = el("input", {
      type: "text",
      placeholder: "User ID (UUID)",
    }) as HTMLInputElement;

    const roleSelect = el("select", { class: "role-select" }) as HTMLSelectElement;
    for (const role of ["member", "admin", "viewer"]) {
      const opt = el("option", { value: role }, role) as HTMLOptionElement;
      roleSelect.appendChild(opt);
    }

    const addBtn = el("button", { class: "primary small" }, "Add Member");
    const errorEl = el("div", { class: "login-error" });

    addBtn.addEventListener("click", async () => {
      const userId = userIdInput.value.trim();
      if (!userId) {
        errorEl.textContent = "User ID is required";
        return;
      }
      errorEl.textContent = "";
      addBtn.disabled = true;

      try {
        await addTeamMember(config, teamId, userId, roleSelect.value);
        userIdInput.value = "";
        renderTeamDetail(teamId);
      } catch (err) {
        errorEl.textContent = `${err}`;
        addBtn.disabled = false;
      }
    });

    const form = el(
      "div",
      { class: "team-add-member-form" },
      el("div", { class: "team-add-member-row" },
        userIdInput,
        roleSelect,
        addBtn
      ),
      errorEl
    );

    content.appendChild(form);
  }

  function renderTeamSessionsList(
    sessions: SessionInfo[],
    teamId: string,
    isTeamAdmin: boolean
  ): void {
    // Create session button (member/admin)
    if (isTeamAdmin || true) {
      const createSessionBtn = el("button", { class: "primary small" }, "New Team Session");
      createSessionBtn.addEventListener("click", () => showCreateTeamSessionModal(teamId));
      content.appendChild(createSessionBtn);
    }

    if (sessions.length === 0) {
      content.appendChild(
        el("div", { class: "admin-empty" }, "No sessions in this team yet.")
      );
      return;
    }

    for (const session of sessions) {
      const isExpired = session.status === "expired";

      const descChildren: (HTMLElement | string)[] = [
        session.description || "(no description)",
      ];
      if (isExpired) {
        descChildren.push(
          el("span", { class: "session-badge expired" }, "expired")
        );
      }

      const card = el(
        "div",
        { class: isExpired ? "session-card session-expired" : "session-card" },
        el("div", {},
          el("div", { class: "session-desc" }, ...descChildren),
          el("div", { class: "session-meta" },
            `ID: ${session.id.slice(0, 8)}... | Created: ${new Date(session.created_at).toLocaleString()}`
          )
        )
      );

      if (!isExpired && onJoinSession) {
        card.addEventListener("click", () => onJoinSession(session));
      }

      content.appendChild(card);
    }
  }

  function showCreateTeamSessionModal(teamId: string): void {
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
      el("h2", {}, "New Team Session"),
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
      createBtn.disabled = true;
      createBtn.textContent = "Creating...";
      errorEl.textContent = "";

      try {
        const session = await createTeamSession(config, teamId, desc);
        if (session.owner_key) {
          storeOwnerKey(session.id, session.owner_key);
        }
        document.body.removeChild(overlay);
        if (onJoinSession) {
          onJoinSession(session);
        } else {
          renderTeamDetail(teamId);
        }
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

    descInput.focus();
  }

  function showEditTeamModal(team: Team, teamId: string): void {
    const nameInput = el("input", {
      type: "text",
      placeholder: "Team name",
    }) as HTMLInputElement;
    nameInput.value = team.name;

    const descInput = el("input", {
      type: "text",
      placeholder: "Description (optional)",
    }) as HTMLInputElement;
    descInput.value = team.description;

    const errorEl = el("div", { class: "login-error" });
    const saveBtn = el("button", { class: "primary" }, "Save");
    const cancelBtn = el("button", {}, "Cancel");

    const modal = el(
      "div",
      { class: "modal" },
      el("h2", {}, "Edit Team"),
      el("div", { class: "field" }, el("label", {}, "Name"), nameInput),
      el("div", { class: "field" }, el("label", {}, "Description"), descInput),
      errorEl,
      el("div", { class: "modal-buttons" }, cancelBtn, saveBtn)
    );

    const overlay = el("div", { class: "modal-overlay" }, modal);
    document.body.appendChild(overlay);

    cancelBtn.addEventListener("click", () => {
      document.body.removeChild(overlay);
    });

    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) document.body.removeChild(overlay);
    });

    saveBtn.addEventListener("click", async () => {
      const name = nameInput.value.trim();
      if (!name) {
        errorEl.textContent = "Name is required";
        return;
      }
      saveBtn.disabled = true;
      saveBtn.textContent = "Saving...";
      errorEl.textContent = "";

      try {
        await updateTeam(config, teamId, name, descInput.value.trim());
        document.body.removeChild(overlay);
        renderTeamDetail(teamId);
      } catch (err) {
        errorEl.textContent = `${err}`;
        saveBtn.disabled = false;
        saveBtn.textContent = "Save";
      }
    });

    nameInput.focus();
  }

  // ── Build view ──

  const view = el(
    "div",
    { class: "admin-view" },
    header,
    tabBar,
    content
  );

  container.appendChild(view);
  renderActiveTab();
}
