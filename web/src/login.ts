import { el, clear } from "./dom";
import { fetchHealth } from "./api";
import type { ConnectionConfig } from "./types";

const STORAGE_KEY = "remora_config";

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
}

export function renderLogin(
  container: HTMLElement,
  onConnect: (config: ConnectionConfig) => void
): void {
  clear(container);

  const saved = loadSavedConfig();

  const urlInput = el("input", {
    type: "text",
    placeholder: "http://your-server:7200",
  }) as HTMLInputElement;
  urlInput.value = saved?.url ?? "";

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

  const errorEl = el("div", { class: "login-error" });
  const submitBtn = el("button", { class: "primary" }, "Connect");

  async function handleConnect(): Promise<void> {
    const url = urlInput.value.trim().replace(/\/+$/, "");
    const token = tokenInput.value.trim();
    const name = nameInput.value.trim();

    if (!url || !token || !name) {
      errorEl.textContent = "All fields are required";
      return;
    }

    if (!/^https?:\/\//i.test(url)) {
      errorEl.textContent = "Server URL must start with http:// or https://";
      return;
    }

    submitBtn.disabled = true;
    submitBtn.textContent = "Connecting...";
    errorEl.textContent = "";

    const healthy = await fetchHealth(url);
    if (!healthy) {
      errorEl.textContent = "Cannot reach server. Check the URL.";
      submitBtn.disabled = false;
      submitBtn.textContent = "Connect";
      return;
    }

    const config: ConnectionConfig = { url, token, name };
    saveConfig(config);
    onConnect(config);
  }

  submitBtn.addEventListener("click", handleConnect);

  // Enter key submits
  for (const input of [urlInput, tokenInput, nameInput]) {
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") handleConnect();
    });
  }

  const card = el(
    "div",
    { class: "login-card" },
    el("h1", {}, "Remora"),
    el("div", { class: "field" }, el("label", {}, "Server URL"), urlInput),
    el("div", { class: "field" }, el("label", {}, "Team Token"), tokenInput),
    el("div", { class: "field" }, el("label", {}, "Display Name"), nameInput),
    errorEl,
    submitBtn
  );

  const view = el("div", { class: "login-view" }, card);
  container.appendChild(view);

  // Focus first empty field
  if (!urlInput.value) urlInput.focus();
  else if (!tokenInput.value) tokenInput.focus();
  else if (!nameInput.value) nameInput.focus();
  else submitBtn.focus();
}
