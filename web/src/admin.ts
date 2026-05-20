import { el, clear } from "./dom";
import {
  adminGetUsage,
  adminGetAnalytics,
  adminListSessions,
  adminUpdateQuota,
  adminDeleteSession,
  adminExpireSession,
  adminListUsers,
  adminUpdateUserRole,
  adminListAuditEvents,
  adminGetSessionEvents,
} from "./api";
import type { ConnectionConfig } from "./types";

type AdminTab = "overview" | "sessions" | "users" | "audit" | "allowlist";

export function renderAdmin(
  container: HTMLElement,
  config: ConnectionConfig,
  onBack: () => void
): void {
  clear(container);

  let activeTab: AdminTab = "overview";

  // ── Header ──

  const backBtn = el("button", {}, "Back to Sessions");
  backBtn.addEventListener("click", onBack);

  const header = el(
    "div",
    { class: "header" },
    el("span", { class: "header-title" }, "Admin Dashboard"),
    el("div", { class: "header-actions" }, backBtn)
  );

  // ── Tabs ──

  const overviewTab = el("button", { class: "tab active" }, "Overview");
  const sessionsTab = el("button", { class: "tab" }, "Sessions");
  const usersTab = el("button", { class: "tab" }, "Users");
  const auditTab = el("button", { class: "tab" }, "Audit Log");
  const allowlistTab = el("button", { class: "tab" }, "Allowlist");
  const allTabs = [overviewTab, sessionsTab, usersTab, auditTab, allowlistTab];
  const tabBar = el("div", { class: "admin-tabs" }, ...allTabs);

  const content = el("div", { class: "admin-content" });

  function switchTab(tab: AdminTab): void {
    activeTab = tab;
    allTabs.forEach((t) => t.classList.remove("active"));
    const tabMap: Record<AdminTab, HTMLElement> = {
      overview: overviewTab,
      sessions: sessionsTab,
      users: usersTab,
      audit: auditTab,
      allowlist: allowlistTab,
    };
    tabMap[tab].classList.add("active");
    renderActiveTab();
  }

  overviewTab.addEventListener("click", () => switchTab("overview"));
  sessionsTab.addEventListener("click", () => switchTab("sessions"));
  usersTab.addEventListener("click", () => switchTab("users"));
  auditTab.addEventListener("click", () => switchTab("audit"));
  allowlistTab.addEventListener("click", () => switchTab("allowlist"));

  // ── Tab renderers ──

  function renderActiveTab(): void {
    clear(content);
    content.appendChild(el("div", { class: "admin-loading" }, "Loading..."));
    if (activeTab === "overview") renderOverviewTab();
    else if (activeTab === "sessions") renderSessionsTab();
    else if (activeTab === "users") renderUsersTab();
    else if (activeTab === "allowlist") renderAllowlistTab();
    else renderAuditTab();
  }

  async function renderOverviewTab(): Promise<void> {
    try {
      const [usage, analytics] = await Promise.all([
        adminGetUsage(config),
        adminGetAnalytics(config),
      ]);
      clear(content);

      // Global stats
      const stats = el(
        "div",
        { class: "admin-stats" },
        statCard("Tokens Today", usage.global.total_tokens_today.toLocaleString()),
        statCard("Total Sessions", String(usage.global.total_sessions)),
        statCard("Active Sessions", String(usage.global.active_sessions))
      );

      // Run analytics
      const runStats = el(
        "div",
        { class: "admin-stats" },
        statCard("Total Runs", String(analytics.total_runs)),
        statCard("Successful", String(analytics.successful)),
        statCard("Failed", String(analytics.failed)),
        statCard("Timed Out", String(analytics.timed_out))
      );

      const avgDuration = el(
        "div",
        { class: "admin-section" },
        el("span", { class: "stat-label" }, `Avg Run Duration: ${analytics.avg_duration_secs.toFixed(1)}s`)
      );

      // Per-session usage table
      const sessionRows = usage.sessions.map((s) =>
        el(
          "tr",
          {},
          el("td", {}, s.session_id.slice(0, 8) + "..."),
          el("td", {}, s.description || "-"),
          el("td", {}, s.tokens_used_today.toLocaleString()),
          el("td", {}, s.daily_token_cap.toLocaleString())
        )
      );

      const usageTable = el(
        "div",
        { class: "admin-table-wrap" },
        el(
          "table",
          { class: "admin-table" },
          el(
            "thead",
            {},
            el("tr", {}, el("th", {}, "Session"), el("th", {}, "Description"), el("th", {}, "Tokens Used"), el("th", {}, "Cap"))
          ),
          el("tbody", {}, ...sessionRows)
        )
      );

      content.appendChild(el("h3", { class: "admin-section-title" }, "Global Usage"));
      content.appendChild(stats);
      content.appendChild(el("h3", { class: "admin-section-title" }, "Run Analytics"));
      content.appendChild(runStats);
      content.appendChild(avgDuration);
      if (usage.sessions.length > 0) {
        content.appendChild(el("h3", { class: "admin-section-title" }, "Per-Session Usage"));
        content.appendChild(usageTable);
      }
    } catch (e) {
      clear(content);
      content.appendChild(el("div", { class: "admin-error" }, `Failed to load: ${e}`));
    }
  }

  async function renderSessionsTab(): Promise<void> {
    try {
      const sessions = await adminListSessions(config);
      clear(content);

      if (sessions.length === 0) {
        content.appendChild(el("div", { class: "admin-empty" }, "No sessions"));
        return;
      }

      const rows = sessions.map((s) => {
        const statusBadge = el(
          "span",
          { class: s.status === "active" ? "admin-badge active" : "admin-badge expired" },
          s.status
        );

        const quotaBtn = el("button", { class: "small" }, "Quota");
        quotaBtn.addEventListener("click", () => {
          const val = prompt("New daily token cap:", String(s.daily_token_cap));
          if (val === null) return;
          const cap = parseInt(val, 10);
          if (isNaN(cap) || cap < 0) { alert("Invalid number"); return; }
          adminUpdateQuota(config, s.id, cap).then(() => renderSessionsTab()).catch((e) => alert(`Error: ${e}`));
        });

        const eventsBtn = el("button", { class: "small" }, "Events");
        eventsBtn.addEventListener("click", () => showSessionEvents(s.id, s.description));

        const actions = el("div", { class: "admin-actions" }, quotaBtn, eventsBtn);

        if (s.status === "active") {
          const expireBtn = el("button", { class: "small" }, "Expire");
          expireBtn.addEventListener("click", () => {
            if (!confirm("Expire this session?")) return;
            adminExpireSession(config, s.id).then(() => renderSessionsTab()).catch((e) => alert(`Error: ${e}`));
          });
          actions.appendChild(expireBtn);
        }

        const delBtn = el("button", { class: "small danger" }, "Delete");
        delBtn.addEventListener("click", () => {
          if (!confirm("Permanently delete this session and all its data?")) return;
          adminDeleteSession(config, s.id).then(() => renderSessionsTab()).catch((e) => alert(`Error: ${e}`));
        });
        actions.appendChild(delBtn);

        return el(
          "tr",
          {},
          el("td", {}, s.id.slice(0, 8) + "..."),
          el("td", {}, s.description || "-"),
          el("td", {}, statusBadge),
          el("td", {}, s.tokens_used_today.toLocaleString()),
          el("td", {}, s.daily_token_cap.toLocaleString()),
          el("td", {}, new Date(s.created_at).toLocaleDateString()),
          el("td", {}, actions)
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
            el("tr", {}, el("th", {}, "ID"), el("th", {}, "Description"), el("th", {}, "Status"), el("th", {}, "Tokens"), el("th", {}, "Cap"), el("th", {}, "Created"), el("th", {}, "Actions"))
          ),
          el("tbody", {}, ...rows)
        )
      );

      content.appendChild(table);
    } catch (e) {
      clear(content);
      content.appendChild(el("div", { class: "admin-error" }, `Failed to load: ${e}`));
    }
  }

  async function showSessionEvents(sessionId: string, description: string): Promise<void> {
    clear(content);
    content.appendChild(el("div", { class: "admin-loading" }, "Loading events..."));

    const backBtn = el("button", { class: "small" }, "Back to Sessions");
    backBtn.addEventListener("click", () => renderSessionsTab());

    try {
      const events = await adminGetSessionEvents(config, sessionId, 50);
      clear(content);

      content.appendChild(backBtn);
      content.appendChild(
        el("h3", { class: "admin-section-title" },
          `Events for: ${description || sessionId.slice(0, 8) + "..."}`
        )
      );

      if (events.length === 0) {
        content.appendChild(
          el("div", { class: "admin-empty" },
            "No events found. The session events endpoint may not be available on this server."
          )
        );
        return;
      }

      const rows = events.map((evt) => {
        const kind = String(evt.kind || "-");
        const author = String(evt.author || "-");
        const timestamp = evt.timestamp ? new Date(String(evt.timestamp)).toLocaleString() : "-";
        const payloadStr = evt.payload ? JSON.stringify(evt.payload) : "-";

        return el(
          "tr",
          {},
          el("td", {}, timestamp),
          el("td", {}, kind),
          el("td", {}, author),
          el("td", { class: "audit-details" }, payloadStr)
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
              el("th", {}, "Time"),
              el("th", {}, "Kind"),
              el("th", {}, "Author"),
              el("th", {}, "Payload")
            )
          ),
          el("tbody", {}, ...rows)
        )
      );

      content.appendChild(table);
    } catch (e) {
      clear(content);
      content.appendChild(backBtn);
      content.appendChild(
        el("div", { class: "admin-error" }, `Failed to load events: ${e}`)
      );
    }
  }

  async function renderUsersTab(): Promise<void> {
    try {
      const users = await adminListUsers(config);
      clear(content);

      if (users.length === 0) {
        content.appendChild(el("div", { class: "admin-empty" }, "No users"));
        return;
      }

      const rows = users.map((u) => {
        const select = el("select", { class: "role-select" }) as HTMLSelectElement;
        for (const role of ["admin", "member", "viewer", "guest"]) {
          const opt = el("option", { value: role }, role) as HTMLOptionElement;
          if (role === u.role) opt.selected = true;
          select.appendChild(opt);
        }
        select.addEventListener("change", () => {
          if (!confirm(`Change ${u.display_name}'s role to "${select.value}"?`)) {
            select.value = u.role;
            return;
          }
          adminUpdateUserRole(config, u.id, select.value)
            .then(() => { u.role = select.value; })
            .catch((e) => { alert(`Error: ${e}`); select.value = u.role; });
        });

        return el(
          "tr",
          {},
          el("td", {}, u.email),
          el("td", {}, u.display_name),
          el("td", {}, select),
          el("td", {}, new Date(u.created_at).toLocaleDateString())
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
            el("tr", {}, el("th", {}, "Email"), el("th", {}, "Name"), el("th", {}, "Role"), el("th", {}, "Created"))
          ),
          el("tbody", {}, ...rows)
        )
      );

      content.appendChild(table);
    } catch (e) {
      clear(content);
      content.appendChild(el("div", { class: "admin-error" }, `Failed to load: ${e}`));
    }
  }

  function renderAllowlistTab(): void {
    clear(content);

    content.appendChild(el("h3", { class: "admin-section-title" }, "Domain Allowlist"));
    content.appendChild(
      el(
        "div",
        { class: "admin-section" },
        "The domain allowlist controls which external URLs participants can fetch via the /fetch command. " +
        "Allowlist management is done through WebSocket commands within a session."
      )
    );

    const commandsTable = el(
      "div",
      { class: "admin-table-wrap" },
      el(
        "table",
        { class: "admin-table" },
        el(
          "thead",
          {},
          el("tr", {}, el("th", {}, "Command"), el("th", {}, "Description"))
        ),
        el(
          "tbody",
          {},
          el("tr", {},
            el("td", {}, "/allowlist"),
            el("td", {}, "View the current allowlist for the session")
          ),
          el("tr", {},
            el("td", {}, "/allowlist_add <domain>"),
            el("td", {}, "Add a domain to the allowlist (e.g., /allowlist_add github.com)")
          ),
          el("tr", {},
            el("td", {}, "/allowlist_remove <domain>"),
            el("td", {}, "Remove a domain from the allowlist")
          ),
          el("tr", {},
            el("td", {}, "/approve <domain> true|false"),
            el("td", {}, "Approve or reject a pending domain request")
          )
        )
      )
    );

    content.appendChild(commandsTable);

    content.appendChild(
      el(
        "div",
        { class: "allowlist-note" },
        "To manage the allowlist, join a session and use these commands in the chat. " +
        "Each session has its own allowlist. Only the session owner can modify it."
      )
    );
  }

  async function renderAuditTab(): Promise<void> {
    const PAGE_SIZE = 50;
    let offset = 0;

    async function loadPage(append: boolean): Promise<void> {
      try {
        const events = await adminListAuditEvents(config, PAGE_SIZE, offset);
        if (!append) clear(content);

        if (events.length === 0 && offset === 0) {
          content.appendChild(el("div", { class: "admin-empty" }, "No audit events"));
          return;
        }

        let tbody = content.querySelector("tbody");
        if (!append || !tbody) {
          const table = el(
            "div",
            { class: "admin-table-wrap" },
            el(
              "table",
              { class: "admin-table" },
              el(
                "thead",
                {},
                el("tr", {}, el("th", {}, "Time"), el("th", {}, "Action"), el("th", {}, "Target"), el("th", {}, "IP"), el("th", {}, "Details"))
              ),
              el("tbody", {})
            )
          );
          content.appendChild(table);
          tbody = content.querySelector("tbody")!;
        }

        for (const evt of events) {
          const detailStr = evt.details ? JSON.stringify(evt.details) : "-";
          const targetStr = evt.target_type + (evt.target_id ? `/${evt.target_id.slice(0, 8)}` : "");
          tbody.appendChild(
            el(
              "tr",
              {},
              el("td", {}, new Date(evt.created_at).toLocaleString()),
              el("td", {}, evt.action),
              el("td", {}, targetStr),
              el("td", {}, evt.ip_address || "-"),
              el("td", { class: "audit-details" }, detailStr)
            )
          );
        }

        // Remove old load more button if present
        const oldBtn = content.querySelector(".load-more-btn");
        if (oldBtn) oldBtn.remove();

        if (events.length === PAGE_SIZE) {
          const loadMoreBtn = el("button", { class: "load-more-btn" }, "Load More");
          loadMoreBtn.addEventListener("click", () => {
            offset += PAGE_SIZE;
            loadPage(true);
          });
          content.appendChild(loadMoreBtn);
        }
      } catch (e) {
        if (!append) clear(content);
        content.appendChild(el("div", { class: "admin-error" }, `Failed to load: ${e}`));
      }
    }

    await loadPage(false);
  }

  // ── Helpers ──

  function statCard(label: string, value: string): HTMLElement {
    return el(
      "div",
      { class: "stat-card" },
      el("div", { class: "stat-value" }, value),
      el("div", { class: "stat-label" }, label)
    );
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
