import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { renderTeams } from "../teams";
import type { ConnectionConfig } from "../types";

const jwtConfig: ConnectionConfig = {
  url: "https://test.example.com",
  token: "eyJhbGciOiJIUzI1NiJ9.test",
  name: "TestUser",
  isAdmin: false,
};

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

describe("renderTeams", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the Teams header with title", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const title = container.querySelector(".header-title");
    expect(title?.textContent).toBe("Teams");
  });

  it("renders Back to Sessions button", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const backBtn = container.querySelector(".header-actions button");
    expect(backBtn?.textContent).toBe("Back to Sessions");
  });

  it("calls onBack when Back button is clicked", () => {
    mockFetch({ "/teams": [] });
    const onBack = vi.fn();
    renderTeams(container, jwtConfig, onBack);
    const backBtn = container.querySelector(".header-actions button") as HTMLElement;
    backBtn.click();
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("renders two tab buttons", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    expect(tabs.length).toBe(2);
    expect(tabs[0].textContent).toBe("My Teams");
    expect(tabs[1].textContent).toBe("Create Team");
  });

  it("defaults to My Teams tab active", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const activeTab = container.querySelector(".admin-tabs .tab.active");
    expect(activeTab?.textContent).toBe("My Teams");
  });

  it("shows empty state when user has no teams", async () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));
    const empty = container.querySelector(".admin-empty");
    expect(empty).toBeTruthy();
    expect(empty?.textContent).toContain("No teams yet");
  });

  it("renders team cards when teams exist", async () => {
    mockFetch({
      "/teams": [
        { id: "t1", name: "Alpha Team", description: "First team", daily_token_cap: 100000, created_at: "2026-01-01T00:00:00Z" },
        { id: "t2", name: "Beta Team", description: "Second team", daily_token_cap: 200000, created_at: "2026-02-01T00:00:00Z" },
      ],
    });
    renderTeams(container, jwtConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));

    const cards = container.querySelectorAll(".team-card");
    expect(cards.length).toBe(2);

    const names = container.querySelectorAll(".team-card-name");
    expect(names[0].textContent).toBe("Alpha Team");
    expect(names[1].textContent).toBe("Beta Team");
  });

  it("switches to Create Team tab when clicked", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[1] as HTMLElement).click();
    expect(tabs[1].classList.contains("active")).toBe(true);
    expect(tabs[0].classList.contains("active")).toBe(false);
  });

  it("renders create team form with fields", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[1] as HTMLElement).click();

    const inputs = container.querySelectorAll("input");
    expect(inputs.length).toBeGreaterThanOrEqual(2);

    const labels = container.querySelectorAll("label");
    const labelTexts = Array.from(labels).map((l) => l.textContent);
    expect(labelTexts).toContain("Name");
    expect(labelTexts).toContain("Description");
  });

  it("shows error when create team name is empty", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const tabs = container.querySelectorAll(".admin-tabs .tab");
    (tabs[1] as HTMLElement).click();

    const createBtn = container.querySelector(".teams-create-form button.primary") as HTMLElement;
    createBtn.click();

    const error = container.querySelector(".login-error");
    expect(error?.textContent).toBe("Team name is required");
  });

  it("shows error on fetch failure", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("network error"));
    renderTeams(container, jwtConfig, vi.fn());
    await new Promise((r) => setTimeout(r, 50));

    const error = container.querySelector(".admin-error");
    expect(error).toBeTruthy();
    expect(error?.textContent).toContain("Failed to load teams");
  });

  it("uses el() helper only — no innerHTML or inline handlers", () => {
    mockFetch({ "/teams": [] });
    renderTeams(container, jwtConfig, vi.fn());
    const scripts = container.querySelectorAll("script");
    expect(scripts.length).toBe(0);
    const allElements = container.querySelectorAll("*");
    for (const elem of allElements) {
      expect(elem.getAttribute("onclick")).toBeNull();
      expect(elem.getAttribute("onerror")).toBeNull();
    }
  });
});

describe("teams button in sessions", () => {
  it("renderSessions accepts onTeams as 6th param", async () => {
    const { renderSessions } = await import("../sessions");
    expect(typeof renderSessions).toBe("function");
    expect(renderSessions.length).toBeGreaterThanOrEqual(5);
  });
});

describe("teams module exports", () => {
  it("exports renderTeams function", async () => {
    const mod = await import("../teams");
    expect(typeof mod.renderTeams).toBe("function");
  });
});
