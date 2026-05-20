import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { renderSessions } from "../sessions";
import type { ConnectionConfig } from "../types";

// Mock fetch globally for session list calls
function mockFetch(data: unknown): void {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = typeof input === "string" ? input : (input as Request).url;
    if (url.includes("/dashboard")) {
      // Dashboard endpoint — return for JWT users
      return {
        ok: true,
        status: 200,
        json: async () => ({ user: { id: "u1", email: "dev@test.com", display_name: "Dev", role: "member", created_at: "2026-01-01" }, sessions: data }),
        text: async () => JSON.stringify(data),
      } as Response;
    }
    if (url.includes("/sessions")) {
      return {
        ok: true,
        status: 200,
        json: async () => data,
        text: async () => JSON.stringify(data),
      } as Response;
    }
    return { ok: false, status: 404, json: async () => ({}), text: async () => "not found" } as Response;
  });
}

describe("renderSessions", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the sessions view with header", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "Dev" }, vi.fn(), vi.fn());
    const view = container.querySelector(".sessions-view");
    expect(view).toBeTruthy();
    const title = container.querySelector(".header-title");
    expect(title?.textContent).toBe("Remora");
  });

  it("shows connected user name in header", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "MyName" }, vi.fn(), vi.fn());
    const status = container.querySelector(".header-status");
    expect(status?.textContent).toContain("MyName");
  });

  it("shows Admin button when config.isAdmin is true", () => {
    const adminConfig: ConnectionConfig = {
      url: "http://test",
      token: "admin-tok",
      name: "Admin",
      isAdmin: true,
    };
    mockFetch([]);
    renderSessions(container, adminConfig, vi.fn(), vi.fn(), vi.fn());
    const adminBtn = container.querySelector("button.admin-btn");
    expect(adminBtn).toBeTruthy();
    expect(adminBtn?.textContent).toBe("Admin");
  });

  it("does NOT show Admin button when config.isAdmin is false", () => {
    const memberConfig: ConnectionConfig = {
      url: "http://test",
      token: "member-tok",
      name: "Member",
      isAdmin: false,
    };
    mockFetch([]);
    renderSessions(container, memberConfig, vi.fn(), vi.fn(), vi.fn());
    const adminBtn = container.querySelector("button.admin-btn");
    expect(adminBtn).toBeNull();
  });

  it("does NOT show Admin button when config.isAdmin is undefined", () => {
    const noAdminConfig: ConnectionConfig = {
      url: "http://test",
      token: "tok",
      name: "User",
    };
    mockFetch([]);
    renderSessions(container, noAdminConfig, vi.fn(), vi.fn(), vi.fn());
    const adminBtn = container.querySelector("button.admin-btn");
    expect(adminBtn).toBeNull();
  });

  it("shows Teams button when onTeams callback is provided", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn(), undefined, vi.fn());
    const teamsBtn = container.querySelector("button.teams-btn");
    expect(teamsBtn).toBeTruthy();
    expect(teamsBtn?.textContent).toBe("Teams");
  });

  it("does NOT show Teams button when onTeams is not provided", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn());
    const teamsBtn = container.querySelector("button.teams-btn");
    expect(teamsBtn).toBeNull();
  });

  it("calls onAdmin when Admin button is clicked", () => {
    const adminConfig: ConnectionConfig = {
      url: "http://test",
      token: "admin-tok",
      name: "Admin",
      isAdmin: true,
    };
    mockFetch([]);
    const onAdmin = vi.fn();
    renderSessions(container, adminConfig, vi.fn(), vi.fn(), onAdmin);
    const adminBtn = container.querySelector("button.admin-btn") as HTMLElement;
    adminBtn.click();
    expect(onAdmin).toHaveBeenCalledOnce();
  });

  it("calls onTeams when Teams button is clicked", () => {
    mockFetch([]);
    const onTeams = vi.fn();
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn(), undefined, onTeams);
    const teamsBtn = container.querySelector("button.teams-btn") as HTMLElement;
    teamsBtn.click();
    expect(onTeams).toHaveBeenCalledOnce();
  });

  it("shows New Session button", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn());
    const newBtn = container.querySelector("button.primary");
    expect(newBtn?.textContent).toBe("New Session");
  });

  it("shows Disconnect button", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn());
    const buttons = container.querySelectorAll("button");
    const disconnectBtn = Array.from(buttons).find((b) => b.textContent === "Disconnect");
    expect(disconnectBtn).toBeTruthy();
  });

  it("calls onDisconnect when Disconnect button is clicked", () => {
    mockFetch([]);
    const onDisconnect = vi.fn();
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), onDisconnect);
    const buttons = container.querySelectorAll("button");
    const disconnectBtn = Array.from(buttons).find((b) => b.textContent === "Disconnect") as HTMLElement;
    disconnectBtn.click();
    expect(onDisconnect).toHaveBeenCalledOnce();
  });

  it("uses el() helper only -- no innerHTML or inline handlers", () => {
    mockFetch([]);
    renderSessions(container, { url: "http://test", token: "tok", name: "User" }, vi.fn(), vi.fn());
    const scripts = container.querySelectorAll("script");
    expect(scripts.length).toBe(0);
    const allElements = container.querySelectorAll("*");
    for (const elem of allElements) {
      expect(elem.getAttribute("onclick")).toBeNull();
      expect(elem.getAttribute("onerror")).toBeNull();
    }
  });
});
