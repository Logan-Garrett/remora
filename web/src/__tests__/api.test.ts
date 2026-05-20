import { describe, it, expect, vi, afterEach } from "vitest";
import { buildWsUrl } from "../api";

describe("buildWsUrl", () => {
  it("converts http to ws", () => {
    const url = buildWsUrl(
      { url: "http://localhost:7200", token: "abc", name: "bob" },
      "session-123"
    );
    expect(url).toMatch(/^ws:\/\/localhost:7200\/sessions\/session-123\?/);
    expect(url).toContain("token=abc");
    expect(url).toContain("name=bob");
  });

  it("converts https to wss", () => {
    const url = buildWsUrl(
      { url: "https://example.com:7200", token: "tok", name: "me" },
      "sid"
    );
    expect(url).toMatch(/^wss:\/\/example\.com:7200\/sessions\/sid\?/);
  });

  it("encodes special characters in token and name", () => {
    const url = buildWsUrl(
      { url: "http://host:7200", token: "a&b=c", name: "my name" },
      "s1"
    );
    // URLSearchParams encodes & and spaces
    expect(url).not.toContain("a&b=c");
    expect(url).toContain("token=a%26b%3Dc");
  });
});

// ── Admin API function tests ──────────────────────────────────────────

describe("admin API functions", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  const config = {
    url: "https://test.example.com",
    token: "admin-test-token",
    name: "Admin",
    isAdmin: true,
  };

  function mockFetchFail(status: number): void {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status,
      json: async () => ({}),
      text: async () => "error",
    } as Response);
  }

  it("adminGetUsage calls correct URL with auth header", async () => {
    const { adminGetUsage } = await import("../api");
    const mockData = { sessions: [], global: { total_tokens_today: 0, total_sessions: 0, active_sessions: 0 } };
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => mockData, text: async () => JSON.stringify(mockData),
    } as Response);

    await adminGetUsage(config);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/usage",
      expect.objectContaining({
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminGetAnalytics calls correct URL with auth header", async () => {
    const { adminGetAnalytics } = await import("../api");
    const mockData = { total_runs: 0, successful: 0, failed: 0, timed_out: 0, avg_duration_secs: 0, runs_by_session: [] };
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => mockData, text: async () => JSON.stringify(mockData),
    } as Response);

    await adminGetAnalytics(config);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/analytics",
      expect.objectContaining({
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminListSessions calls correct URL", async () => {
    const { adminListSessions } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => [], text: async () => "[]",
    } as Response);

    await adminListSessions(config);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/sessions",
      expect.objectContaining({
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminListUsers calls correct URL", async () => {
    const { adminListUsers } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => [], text: async () => "[]",
    } as Response);

    await adminListUsers(config);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/users",
      expect.objectContaining({
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminUpdateUserRole calls PUT with correct URL and body", async () => {
    const { adminUpdateUserRole } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 204, json: async () => ({}), text: async () => "",
    } as Response);

    await adminUpdateUserRole(config, "user-123", "viewer");
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/users/user-123/role",
      expect.objectContaining({
        method: "PUT",
        body: JSON.stringify({ role: "viewer" }),
      })
    );
  });

  it("adminUpdateQuota calls PUT with correct URL and body", async () => {
    const { adminUpdateQuota } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 204, json: async () => ({}), text: async () => "",
    } as Response);

    await adminUpdateQuota(config, "sess-456", 50000);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/sessions/sess-456/quota",
      expect.objectContaining({
        method: "PUT",
        body: JSON.stringify({ daily_token_cap: 50000 }),
      })
    );
  });

  it("adminDeleteSession calls DELETE with correct URL", async () => {
    const { adminDeleteSession } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 204, json: async () => ({}), text: async () => "",
    } as Response);

    await adminDeleteSession(config, "sess-789");
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/sessions/sess-789",
      expect.objectContaining({
        method: "DELETE",
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminExpireSession calls POST with correct URL", async () => {
    const { adminExpireSession } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 204, json: async () => ({}), text: async () => "",
    } as Response);

    await adminExpireSession(config, "sess-101");
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/sessions/sess-101/expire",
      expect.objectContaining({
        method: "POST",
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminListAuditEvents calls correct URL with limit and offset", async () => {
    const { adminListAuditEvents } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => [], text: async () => "[]",
    } as Response);

    await adminListAuditEvents(config, 25, 10);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/admin/audit?limit=25&offset=10",
      expect.objectContaining({
        headers: { Authorization: "Bearer admin-test-token" },
      })
    );
  });

  it("adminGetUsage throws on non-ok response", async () => {
    const { adminGetUsage } = await import("../api");
    mockFetchFail(403);
    await expect(adminGetUsage(config)).rejects.toThrow("Admin usage: 403");
  });

  it("adminListUsers throws on non-ok response", async () => {
    const { adminListUsers } = await import("../api");
    mockFetchFail(500);
    await expect(adminListUsers(config)).rejects.toThrow("Admin users: 500");
  });
});

// ── Teams API function tests ──────────────────────────────────────────

describe("teams API functions", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  const config = {
    url: "https://test.example.com",
    token: "eyJhbGciOiJIUzI1NiJ9.test-jwt",
    name: "TestUser",
    isAdmin: false,
  };

  it("listTeams calls correct URL with auth header", async () => {
    const { listTeams } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 200, json: async () => [], text: async () => "[]",
    } as Response);

    await listTeams(config);
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/teams",
      expect.objectContaining({
        headers: { Authorization: "Bearer eyJhbGciOiJIUzI1NiJ9.test-jwt" },
      })
    );
  });

  it("createTeam calls POST with correct body", async () => {
    const { createTeam } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 201,
      json: async () => ({ id: "t1", name: "My Team", description: "desc", daily_token_cap: 100000, created_at: "2026-01-01" }),
      text: async () => "{}",
    } as Response);

    await createTeam(config, "My Team", "desc");
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/teams",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ name: "My Team", description: "desc" }),
      })
    );
  });

  it("listTeams throws on 401", async () => {
    const { listTeams } = await import("../api");
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false, status: 401, json: async () => ({}), text: async () => "",
    } as Response);

    await expect(listTeams(config)).rejects.toThrow("JWT or API key required");
  });

  it("createTeam throws on 409 (duplicate name)", async () => {
    const { createTeam } = await import("../api");
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false, status: 409, json: async () => ({}), text: async () => "",
    } as Response);

    await expect(createTeam(config, "Dup", "desc")).rejects.toThrow("Team name already exists");
  });

  it("deleteTeam calls DELETE with correct URL", async () => {
    const { deleteTeam } = await import("../api");
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true, status: 204, json: async () => ({}), text: async () => "",
    } as Response);

    await deleteTeam(config, "team-123");
    expect(spy).toHaveBeenCalledWith(
      "https://test.example.com/teams/team-123",
      expect.objectContaining({
        method: "DELETE",
      })
    );
  });
});
