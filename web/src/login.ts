import { el, clear } from "./dom";
import { fetchHealth, authLogin, authRegister } from "./api";
import type { ConnectionConfig, AuthResponse } from "./types";

const STORAGE_KEY = "remora_config";
const REFRESH_KEY = "remora_refresh_token";

export function loadSavedConfig(): ConnectionConfig | null {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as ConnectionConfig;
    if (parsed.url && parsed.token && parsed.name) return parsed;
  } catch {
    // ignore
  }
  return null;
}

export function saveConfig(config: ConnectionConfig): void {
  sessionStorage.setItem(STORAGE_KEY, JSON.stringify(config));
}

export function clearConfig(): void {
  sessionStorage.removeItem(STORAGE_KEY);
  sessionStorage.removeItem(REFRESH_KEY);
}

// ── OAuth popup ──────────────────────────────────────────────────────────────

/** Open a popup for OAuth and listen for the postMessage callback. */
function openOAuthPopup(
  serverUrl: string,
  provider: "github" | "google",
  onSuccess: (auth: AuthResponse) => void,
  onError: (msg: string) => void
): void {
  const origin = window.location.origin;
  const url = `${serverUrl}/auth/oauth/${provider}?origin=${encodeURIComponent(origin)}`;
  const w = 500;
  const h = 600;
  const left = window.screenX + (window.outerWidth - w) / 2;
  const top = window.screenY + (window.outerHeight - h) / 2;
  const popup = window.open(
    url,
    "remora-oauth",
    `width=${w},height=${h},left=${left},top=${top},popup=yes`
  );

  if (!popup) {
    onError("Popup blocked. Please allow popups for this site.");
    return;
  }

  function handleMessage(event: MessageEvent): void {
    // Validate the message origin matches the server
    const serverOrigin = new URL(serverUrl).origin;
    if (event.origin !== serverOrigin) return;
    if (!event.data || event.data.type !== "remora-oauth-success") return;

    window.removeEventListener("message", handleMessage);
    clearInterval(pollTimer);

    const data = event.data as AuthResponse & { type: string };
    onSuccess({
      access_token: data.access_token,
      refresh_token: data.refresh_token,
      user: data.user,
    });
  }

  window.addEventListener("message", handleMessage);

  // Poll in case the popup closes without posting (user cancelled)
  const pollTimer = setInterval(() => {
    if (popup.closed) {
      clearInterval(pollTimer);
      window.removeEventListener("message", handleMessage);
      // Only report error if we haven't already succeeded
    }
  }, 500);
}

// ── Render ────────────────────────────────────────────────────────────────────

type AuthMode = "token" | "login" | "register";

export function renderLogin(
  container: HTMLElement,
  onConnect: (config: ConnectionConfig) => void
): void {
  clear(container);

  const saved = loadSavedConfig();
  let mode: AuthMode = "token";

  // ── Shared: Server URL ──

  const urlInput = el("input", {
    type: "text",
    placeholder: "http://your-server:7200",
  }) as HTMLInputElement;
  urlInput.value = saved?.url ?? "";

  const errorEl = el("div", { class: "login-error" });

  // ── Token mode fields ──

  const tokenInput = el("input", {
    type: "password",
    placeholder: "your-team-token",
  }) as HTMLInputElement;
  tokenInput.value = saved?.token ?? "";

  const nameInput = el("input", {
    type: "text",
    placeholder: "your-name",
  }) as HTMLInputElement;
  nameInput.value = saved?.name ?? "";

  // ── Email/password mode fields ──

  const emailInput = el("input", {
    type: "email",
    placeholder: "email@example.com",
  }) as HTMLInputElement;

  const passwordInput = el("input", {
    type: "password",
    placeholder: "password (min 8 chars)",
  }) as HTMLInputElement;

  const registerNameInput = el("input", {
    type: "text",
    placeholder: "display name",
  }) as HTMLInputElement;

  // ── Buttons ──

  const submitBtn = el("button", { class: "primary" }, "Connect");

  const githubBtn = el("button", { class: "oauth-btn github" }, "Continue with GitHub");
  const googleBtn = el("button", { class: "oauth-btn google" }, "Continue with Google");

  // ── Tab buttons ──

  const tokenTab = el("button", { class: "tab active" }, "Token");
  const loginTab = el("button", { class: "tab" }, "Login");
  const registerTab = el("button", { class: "tab" }, "Register");
  const tabs = el("div", { class: "login-tabs" }, tokenTab, loginTab, registerTab);

  // ── Dynamic form area ──

  const formArea = el("div", { class: "login-form-area" });

  function renderForm(): void {
    clear(formArea);
    clear(errorEl);
    const tabs = [tokenTab, loginTab, registerTab];
    tabs.forEach((t) => t.classList.remove("active"));

    if (mode === "token") {
      tokenTab.classList.add("active");
      submitBtn.textContent = "Connect";
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Team Token"), tokenInput)
      );
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Display Name"), nameInput)
      );
    } else if (mode === "login") {
      loginTab.classList.add("active");
      submitBtn.textContent = "Sign In";
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Email"), emailInput)
      );
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Password"), passwordInput)
      );
    } else {
      registerTab.classList.add("active");
      submitBtn.textContent = "Create Account";
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Email"), emailInput)
      );
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Display Name"), registerNameInput)
      );
      formArea.appendChild(
        el("div", { class: "field" }, el("label", {}, "Password"), passwordInput)
      );
    }
  }

  tokenTab.addEventListener("click", () => {
    mode = "token";
    renderForm();
  });
  loginTab.addEventListener("click", () => {
    mode = "login";
    renderForm();
  });
  registerTab.addEventListener("click", () => {
    mode = "register";
    renderForm();
  });

  // ── Helpers ──

  function getServerUrl(): string {
    return urlInput.value.trim().replace(/\/+$/, "");
  }

  function setError(msg: string): void {
    errorEl.textContent = msg;
  }

  function setLoading(loading: boolean): void {
    submitBtn.disabled = loading;
    githubBtn.disabled = loading;
    googleBtn.disabled = loading;
  }

  async function validateServer(): Promise<boolean> {
    const url = getServerUrl();
    if (!url) {
      setError("Server URL is required");
      return false;
    }
    if (!/^https?:\/\//i.test(url)) {
      setError("Server URL must start with http:// or https://");
      return false;
    }
    const healthy = await fetchHealth(url);
    if (!healthy) {
      setError("Cannot reach server. Check the URL.");
      return false;
    }
    return true;
  }

  function completeLogin(config: ConnectionConfig, refreshToken?: string): void {
    saveConfig(config);
    if (refreshToken) {
      sessionStorage.setItem(REFRESH_KEY, refreshToken);
    }
    onConnect(config);
  }

  // ── Handlers ──

  async function handleSubmit(): Promise<void> {
    setError("");
    setLoading(true);

    if (!(await validateServer())) {
      setLoading(false);
      return;
    }

    const url = getServerUrl();

    try {
      if (mode === "token") {
        const token = tokenInput.value.trim();
        const name = nameInput.value.trim();
        if (!token || !name) {
          setError("Token and display name are required");
          setLoading(false);
          return;
        }
        completeLogin({ url, token, name, isAdmin: true });
      } else if (mode === "login") {
        const email = emailInput.value.trim();
        const password = passwordInput.value;
        if (!email || !password) {
          setError("Email and password are required");
          setLoading(false);
          return;
        }
        const auth = await authLogin(url, email, password);
        completeLogin(
          { url, token: auth.access_token, name: auth.user.display_name, isAdmin: auth.user.role === "admin" },
          auth.refresh_token
        );
      } else {
        const email = emailInput.value.trim();
        const displayName = registerNameInput.value.trim();
        const password = passwordInput.value;
        if (!email || !displayName || !password) {
          setError("All fields are required");
          setLoading(false);
          return;
        }
        if (password.length < 8) {
          setError("Password must be at least 8 characters");
          setLoading(false);
          return;
        }
        await authRegister(url, email, displayName, password);
        // Auto-login after registration
        const auth = await authLogin(url, email, password);
        completeLogin(
          { url, token: auth.access_token, name: auth.user.display_name, isAdmin: auth.user.role === "admin" },
          auth.refresh_token
        );
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "An error occurred");
      setLoading(false);
    }
  }

  function handleOAuth(provider: "github" | "google"): void {
    const url = getServerUrl();
    if (!url || !/^https?:\/\//i.test(url)) {
      setError("Enter a valid server URL first");
      return;
    }
    setError("");
    setLoading(true);

    openOAuthPopup(
      url,
      provider,
      (auth) => {
        setLoading(false);
        completeLogin(
          { url, token: auth.access_token, name: auth.user.display_name, isAdmin: auth.user.role === "admin" },
          auth.refresh_token
        );
      },
      (msg) => {
        setLoading(false);
        setError(msg);
      }
    );
  }

  // ── Event listeners ──

  submitBtn.addEventListener("click", handleSubmit);
  githubBtn.addEventListener("click", () => handleOAuth("github"));
  googleBtn.addEventListener("click", () => handleOAuth("google"));

  // Enter key submits
  for (const input of [urlInput, tokenInput, nameInput, emailInput, passwordInput, registerNameInput]) {
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") handleSubmit();
    });
  }

  // ── Build card ──

  renderForm();

  const divider = el(
    "div",
    { class: "login-divider" },
    el("span", {}, "or")
  );

  const oauthSection = el(
    "div",
    { class: "oauth-section" },
    githubBtn,
    googleBtn
  );

  const card = el(
    "div",
    { class: "login-card" },
    el("h1", {}, "Remora"),
    el("div", { class: "field" }, el("label", {}, "Server URL"), urlInput),
    tabs,
    formArea,
    errorEl,
    submitBtn,
    divider,
    oauthSection
  );

  const view = el("div", { class: "login-view" }, card);
  container.appendChild(view);

  // PWA install prompt
  let deferredPrompt: Event | null = null;
  window.addEventListener(
    "beforeinstallprompt",
    (e) => {
      e.preventDefault();
      deferredPrompt = e;
      const installBtn = el("button", { class: "secondary install-btn" }, "Install App");
      installBtn.addEventListener("click", () => {
        (deferredPrompt as unknown as { prompt(): void })?.prompt();
        deferredPrompt = null;
        installBtn.remove();
      });
      if (card) card.appendChild(installBtn);
    },
    { once: true }
  );

  // Focus first empty field
  if (!urlInput.value) urlInput.focus();
  else if (mode === "token" && !tokenInput.value) tokenInput.focus();
  else if (mode === "token" && !nameInput.value) nameInput.focus();
  else submitBtn.focus();
}
