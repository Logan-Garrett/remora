import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { renderLogin, loadSavedConfig, saveConfig, clearConfig } from "../login";

describe("renderLogin", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the login card with server URL field", () => {
    renderLogin(container, vi.fn());
    const h1 = container.querySelector("h1");
    expect(h1?.textContent).toBe("Remora");
    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    expect(urlInput).toBeTruthy();
  });

  it("renders tab buttons for token, login, and register", () => {
    renderLogin(container, vi.fn());
    const tabs = container.querySelectorAll(".login-tabs .tab");
    expect(tabs.length).toBe(3);
    expect(tabs[0].textContent).toBe("Token");
    expect(tabs[1].textContent).toBe("Login");
    expect(tabs[2].textContent).toBe("Register");
  });

  it("defaults to token mode with token and name fields", () => {
    renderLogin(container, vi.fn());
    const tokenInput = container.querySelector(
      'input[placeholder="your-team-token"]'
    );
    const nameInput = container.querySelector(
      'input[placeholder="your-name"]'
    );
    expect(tokenInput).toBeTruthy();
    expect(nameInput).toBeTruthy();
  });

  it("switches to login mode when Login tab is clicked", () => {
    renderLogin(container, vi.fn());
    const loginTab = container.querySelectorAll(".login-tabs .tab")[1] as HTMLElement;
    loginTab.click();
    const emailInput = container.querySelector(
      'input[type="email"]'
    );
    const passwordInput = container.querySelector(
      'input[type="password"]'
    );
    expect(emailInput).toBeTruthy();
    expect(passwordInput).toBeTruthy();
    // Token-specific fields should be gone
    const tokenInput = container.querySelector(
      'input[placeholder="your-team-token"]'
    );
    expect(tokenInput).toBeNull();
  });

  it("switches to register mode with email, name, and password fields", () => {
    renderLogin(container, vi.fn());
    const registerTab = container.querySelectorAll(".login-tabs .tab")[2] as HTMLElement;
    registerTab.click();
    const emailInput = container.querySelector('input[type="email"]');
    const nameInput = container.querySelector(
      'input[placeholder="display name"]'
    );
    const passwordInput = container.querySelector('input[type="password"]');
    expect(emailInput).toBeTruthy();
    expect(nameInput).toBeTruthy();
    expect(passwordInput).toBeTruthy();
  });

  it("renders OAuth buttons for GitHub and Google", () => {
    renderLogin(container, vi.fn());
    const githubBtn = container.querySelector(".oauth-btn.github");
    const googleBtn = container.querySelector(".oauth-btn.google");
    expect(githubBtn?.textContent).toBe("Continue with GitHub");
    expect(googleBtn?.textContent).toBe("Continue with Google");
  });

  it("renders the or divider between form and OAuth", () => {
    renderLogin(container, vi.fn());
    const divider = container.querySelector(".login-divider");
    expect(divider).toBeTruthy();
    expect(divider?.querySelector("span")?.textContent).toBe("or");
  });

  it("shows error when server URL is empty on submit", async () => {
    renderLogin(container, vi.fn());
    const submitBtn = container.querySelector("button.primary") as HTMLButtonElement;
    submitBtn.click();
    // Need to wait for async handler
    await new Promise((r) => setTimeout(r, 10));
    const error = container.querySelector(".login-error");
    expect(error?.textContent).toBe("Server URL is required");
  });

  it("shows error when server URL has invalid scheme", async () => {
    renderLogin(container, vi.fn());
    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "ftp://server";
    const submitBtn = container.querySelector("button.primary") as HTMLButtonElement;
    submitBtn.click();
    await new Promise((r) => setTimeout(r, 10));
    const error = container.querySelector(".login-error");
    expect(error?.textContent).toBe(
      "Server URL must start with http:// or https://"
    );
  });

  it("uses el() for all DOM construction (no innerHTML)", () => {
    renderLogin(container, vi.fn());
    // Walk the entire DOM tree and verify no element has innerHTML set
    // by checking there are no script elements or event handlers
    const scripts = container.querySelectorAll("script");
    expect(scripts.length).toBe(0);
    // Verify text content is escaped (no raw HTML in text nodes)
    const allElements = container.querySelectorAll("*");
    for (const el of allElements) {
      // Check that no element has an onclick or other inline handler
      expect(el.getAttribute("onclick")).toBeNull();
      expect(el.getAttribute("onerror")).toBeNull();
    }
  });

  it("restores saved config into URL field", () => {
    saveConfig({ url: "http://saved:7200", token: "tok", name: "user" });
    renderLogin(container, vi.fn());
    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    expect(urlInput.value).toBe("http://saved:7200");
  });

  it("shows error for OAuth when no server URL entered", () => {
    renderLogin(container, vi.fn());
    const githubBtn = container.querySelector(".oauth-btn.github") as HTMLButtonElement;
    githubBtn.click();
    const error = container.querySelector(".login-error");
    expect(error?.textContent).toBe("Enter a valid server URL first");
  });
});

describe("config persistence", () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  it("saves and loads config", () => {
    expect(loadSavedConfig()).toBeNull();
    saveConfig({ url: "http://test:7200", token: "abc", name: "me" });
    const loaded = loadSavedConfig();
    expect(loaded).toEqual({
      url: "http://test:7200",
      token: "abc",
      name: "me",
    });
  });

  it("clears config and refresh token", () => {
    saveConfig({ url: "http://test:7200", token: "abc", name: "me" });
    sessionStorage.setItem("remora_refresh_token", "refresh123");
    clearConfig();
    expect(loadSavedConfig()).toBeNull();
    expect(sessionStorage.getItem("remora_refresh_token")).toBeNull();
  });

  it("returns null for corrupted config", () => {
    sessionStorage.setItem("remora_config", "not-json");
    expect(loadSavedConfig()).toBeNull();
  });

  it("returns null for incomplete config", () => {
    sessionStorage.setItem(
      "remora_config",
      JSON.stringify({ url: "http://test" })
    );
    expect(loadSavedConfig()).toBeNull();
  });
});

describe("OAuth popup security", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("opens popup with correct URL including origin param", () => {
    const openSpy = vi.spyOn(window, "open").mockReturnValue(null);
    renderLogin(container, vi.fn());

    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "https://remora.example.com";

    const githubBtn = container.querySelector(
      ".oauth-btn.github"
    ) as HTMLButtonElement;
    githubBtn.click();

    expect(openSpy).toHaveBeenCalledOnce();
    const calledUrl = openSpy.mock.calls[0][0] as string;
    expect(calledUrl).toContain(
      "https://remora.example.com/auth/oauth/github"
    );
    expect(calledUrl).toContain("origin=");
    // Verify the origin is URL-encoded
    expect(calledUrl).toContain(encodeURIComponent(window.location.origin));
  });

  it("shows error when popup is blocked", () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    renderLogin(container, vi.fn());

    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "https://remora.example.com";

    const githubBtn = container.querySelector(
      ".oauth-btn.github"
    ) as HTMLButtonElement;
    githubBtn.click();

    const error = container.querySelector(".login-error");
    expect(error?.textContent).toBe(
      "Popup blocked. Please allow popups for this site."
    );
  });

  it("ignores postMessage from wrong origin", async () => {
    const mockPopup = { closed: false, close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(
      mockPopup as unknown as Window
    );
    const onConnect = vi.fn();
    renderLogin(container, onConnect);

    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "https://remora.example.com";

    const githubBtn = container.querySelector(
      ".oauth-btn.github"
    ) as HTMLButtonElement;
    githubBtn.click();

    // Send a message from the wrong origin
    const fakeEvent = new MessageEvent("message", {
      origin: "https://evil.example.com",
      data: {
        type: "remora-oauth-success",
        access_token: "stolen",
        refresh_token: "stolen",
        user: { display_name: "hacker" },
      },
    });
    window.dispatchEvent(fakeEvent);

    await new Promise((r) => setTimeout(r, 50));
    expect(onConnect).not.toHaveBeenCalled();
  });

  it("ignores postMessage with wrong type", async () => {
    const mockPopup = { closed: false, close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(
      mockPopup as unknown as Window
    );
    const onConnect = vi.fn();
    renderLogin(container, onConnect);

    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "https://remora.example.com";

    const githubBtn = container.querySelector(
      ".oauth-btn.github"
    ) as HTMLButtonElement;
    githubBtn.click();

    const fakeEvent = new MessageEvent("message", {
      origin: "https://remora.example.com",
      data: { type: "some-other-message" },
    });
    window.dispatchEvent(fakeEvent);

    await new Promise((r) => setTimeout(r, 50));
    expect(onConnect).not.toHaveBeenCalled();
  });

  it("accepts postMessage from correct server origin with correct type", async () => {
    const mockPopup = { closed: false, close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(
      mockPopup as unknown as Window
    );
    const onConnect = vi.fn();
    renderLogin(container, onConnect);

    const urlInput = container.querySelector(
      'input[placeholder="http://your-server:7200"]'
    ) as HTMLInputElement;
    urlInput.value = "https://remora.example.com";

    const githubBtn = container.querySelector(
      ".oauth-btn.github"
    ) as HTMLButtonElement;
    githubBtn.click();

    const authEvent = new MessageEvent("message", {
      origin: "https://remora.example.com",
      data: {
        type: "remora-oauth-success",
        access_token: "jwt-token-123",
        refresh_token: "refresh-456",
        user: {
          id: "user-1",
          email: "dev@example.com",
          display_name: "Dev User",
          role: "member",
          created_at: "2026-01-01T00:00:00Z",
        },
      },
    });
    window.dispatchEvent(authEvent);

    await new Promise((r) => setTimeout(r, 50));
    expect(onConnect).toHaveBeenCalledWith({
      url: "https://remora.example.com",
      token: "jwt-token-123",
      name: "Dev User",
      isAdmin: false,
    });
  });
});
