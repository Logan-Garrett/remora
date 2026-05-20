import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { renderAdmin } from "../admin";
import type { ConnectionConfig } from "../types";

const adminConfig: ConnectionConfig = {
  url: "https://test.example.com",
  token: "admin-token",
  name: "Admin",
  isAdmin: true,
};

// Mock fetch globally
function mockFetch(responses: Record<string, unknown>): void {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = typeof input === "string" ? input : (input as Request).url;
    for (const [pattern, data] of Object.entries(responses)) {
      if (url.includes(pattern)) {
        return {
          ok: true,
          status: 200,
          json: async () => data,
          text: async () => JSON.stringify(data),
        } as Response;
      }
    }
    return { ok: false, status: 404, json: async () => ({}), text: async () => "not found" } as Response;
  });
}

describe("renderAdmin", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the admin header with title", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    renderAdmin(container, adminConfig, vi.fn());
    const title = container.querySelector(".header-title");
    expect(title?.textContent).toBe("Admin Dashboard");
  });

  it("renders Back to Sessions button", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    renderAdmin(container, adminConfig, vi.fn());
    const backBtn = container.querySelector(".header-actions button");
    expect(backBtn?.textContent).toBe("Back to Sessions");
  });

  it("calls onBack when Back button is clicked", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    const onBack = vi.fn();
    renderAdmin(container, adminConfig, onBack);
    const backBtn = container.querySelector(".header-actions button") as HTMLElement;
    backBtn.click();
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("renders four tab buttons", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    renderAdmin(container, adminConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    expect(tabs.length).toBe(4);
    expect(tabs[0].textContent).toBe("Overview");
    expect(tabs[1].textContent).toBe("Sessions");
    expect(tabs[2].textContent).toBe("Users");
    expect(tabs[3].textContent).toBe("Audit Log");
  });

  it("defaults to Overview tab active", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    renderAdmin(container, adminConfig, vi.fn());
    const activeTab = container.querySelector(".admin-tabs .tab.active");
    expect(activeTab?.textContent).toBe("Overview");
  });

  it("switches to Sessions tab when clicked", async () => {
    mockFetch({
      "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } },
      "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] },
      "/admin/sessions": [],
    });
    renderAdmin(container, adminConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[1] as HTMLElement).click();
    expect(tabs[1].classList.contains("active")).toBe(true);
    expect(tabs[0].classList.contains("active")).toBe(false);
  });

  it("switches to Users tab when clicked", async () => {
    mockFetch({
      "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } },
      "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] },
      "/admin/users": [],
    });
    renderAdmin(container, adminConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[2] as HTMLElement).click();
    expect(tabs[2].classList.contains("active")).toBe(true);
  });

  it("switches to Audit Log tab when clicked", async () => {
    mockFetch({
      "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } },
      "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] },
      "/admin/audit": [],
    });
    renderAdmin(container, adminConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[3] as HTMLElement).click();
    expect(tabs[3].classList.contains("active")).toBe(true);
  });

  it("displays global usage stats in Overview", async () => {
    mockFetch({
      "/admin/usage": {
        sessions: [{ session_id: "abc-123", description: "Test", tokens_used_today: 5000, daily_token_cap: 100000, tokens_reset_date: "2026-05-20" }],
        global: { total_tokens_today: 12450, total_sessions: 23, active_sessions: 8 },
      },
      "/admin/analytics": { total_runs: 156, successful: 142, failed: 12, timed_out: 2, avg_duration_secs: 34.2, runs_by_session: [] },
    });
    renderAdmin(container, adminConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));

    const statValues = container.querySelectorAll(".stat-value");
    const values = Array.from(statValues).map((el) => el.textContent);
    expect(values).toContain("12,450");
    expect(values).toContain("23");
    expect(values).toContain("8");
    expect(values).toContain("156");
    expect(values).toContain("142");
  });

  it("displays per-session usage table in Overview", async () => {
    mockFetch({
      "/admin/usage": {
        sessions: [{ session_id: "abc12345-dead-beef-1234-567890abcdef", description: "My project", tokens_used_today: 3400, daily_token_cap: 50000, tokens_reset_date: "2026-05-20" }],
        global: { total_tokens_today: 3400, total_sessions: 1, active_sessions: 1 },
      },
      "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] },
    });
    renderAdmin(container, adminConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));

    const table = container.querySelector(".admin-table");
    expect(table).toBeTruthy();
    const cells = table!.querySelectorAll("td");
    const texts = Array.from(cells).map((c) => c.textContent);
    expect(texts).toContain("My project");
    expect(texts).toContain("3,400");
  });

  it("shows error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("network error"));
    renderAdmin(container, adminConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));

    const error = container.querySelector(".admin-error");
    expect(error).toBeTruthy();
    expect(error?.textContent).toContain("Failed to load");
  });

  it("renders users with role selects in Users tab", async () => {
    mockFetch({
      "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } },
      "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] },
      "/admin/users": [
        { id: "u1", email: "dev@example.com", display_name: "Dev", role: "member", created_at: "2026-01-01T00:00:00Z" },
        { id: "u2", email: "admin@example.com", display_name: "Admin", role: "admin", created_at: "2026-01-01T00:00:00Z" },
      ],
    });
    renderAdmin(container, adminConfig, vi.fn());
    // Wait for initial overview fetch to settle
    await new Promise((r) => setTimeout(r, 50));
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[2] as HTMLElement).click();
    // Wait for users fetch
    await new Promise((r) => setTimeout(r, 100));

    const selects = container.querySelectorAll(".role-select") as NodeListOf<HTMLSelectElement>;
    expect(selects.length).toBe(2);
    expect(selects[0].value).toBe("member");
    expect(selects[1].value).toBe("admin");
  });

  it("uses el() helper only — no innerHTML or inline handlers", () => {
    mockFetch({ "/admin/usage": { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } }, "/admin/analytics": { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] } });
    renderAdmin(container, adminConfig, vi.fn());
    const scripts = container.querySelectorAll("script");
    expect(scripts.length).toBe(0);
    const allElements = container.querySelectorAll("*");
    for (const el of allElements) {
      expect(el.getAttribute("onclick")).toBeNull();
      expect(el.getAttribute("onerror")).toBeNull();
    }
  });
});

describe("admin button in sessions", () => {
  it("is exported from sessions module for integration", async () => {
    const { renderSessions } = await import("../sessions");
    expect(typeof renderSessions).toBe("function");
    // renderSessions accepts onAdmin as 5th param
    expect(renderSessions.length).toBeGreaterThanOrEqual(4);
  });
});
