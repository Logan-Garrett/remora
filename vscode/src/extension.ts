import * as vscode from "vscode";
import { ChatPanelProvider } from "./chatPanel";
import type { ConnectionConfig, SessionInfo } from "./types";

let chatProvider: ChatPanelProvider;
let statusBarItem: vscode.StatusBarItem;
let currentConfig: ConnectionConfig | undefined;
let secrets: vscode.SecretStorage;

export function activate(context: vscode.ExtensionContext): void {
  secrets = context.secrets;
  chatProvider = new ChatPanelProvider(context.extensionUri);

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      ChatPanelProvider.viewType,
      chatProvider
    )
  );

  // Migrate token from plaintext settings to SecretStorage (one-time)
  migrateTokenToSecrets().catch(() => {});

  // Status bar item
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    50
  );
  statusBarItem.text = "$(plug) Remora";
  statusBarItem.tooltip = "Remora: Click to connect";
  statusBarItem.command = "remora.connect";
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  // Commands
  context.subscriptions.push(
    vscode.commands.registerCommand("remora.connect", cmdConnect),
    vscode.commands.registerCommand("remora.sessions", cmdSessions),
    vscode.commands.registerCommand("remora.join", cmdJoin),
    vscode.commands.registerCommand("remora.leave", cmdLeave),
    vscode.commands.registerCommand("remora.send", cmdSend),
    vscode.commands.registerCommand("remora.setToken", cmdSetToken)
  );
}

export function deactivate(): void {
  chatProvider?.leaveSession();
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

async function cmdConnect(): Promise<void> {
  const config = vscode.workspace.getConfiguration("remora");

  let serverUrl = config.get<string>("serverUrl") || "";
  let token = await getToken();
  let displayName = config.get<string>("displayName") || "";

  // Prompt for missing settings
  if (!serverUrl) {
    const input = await vscode.window.showInputBox({
      title: "Remora Server URL",
      prompt: "Enter the Remora server URL",
      placeHolder: "http://localhost:7200",
      ignoreFocusOut: true,
    });
    if (!input) return;
    serverUrl = input.trim();
  }

  if (!token) {
    const input = await vscode.window.showInputBox({
      title: "Remora Token",
      prompt: "Enter your team authentication token",
      password: true,
      ignoreFocusOut: true,
    });
    if (!input) return;
    token = input.trim();
    await secrets.store("remora.token", token);
  }

  if (!displayName) {
    const input = await vscode.window.showInputBox({
      title: "Display Name",
      prompt: "Enter your display name for sessions",
      placeHolder: "your-name",
      ignoreFocusOut: true,
    });
    if (!input) return;
    displayName = input.trim();
  }

  // Health check
  const healthy = await checkHealth(serverUrl);
  if (!healthy) {
    vscode.window.showErrorMessage(
      `Remora: Cannot reach server at ${serverUrl}`
    );
    return;
  }

  currentConfig = { url: serverUrl, token, name: displayName };
  statusBarItem.text = "$(plug) Remora: Connected";
  statusBarItem.tooltip = `Connected to ${serverUrl} as ${displayName}`;
  vscode.window.showInformationMessage(
    `Remora: Connected to ${serverUrl} as ${displayName}`
  );
}

async function cmdSetToken(): Promise<void> {
  const input = await vscode.window.showInputBox({
    title: "Remora Token",
    prompt: "Enter your team authentication token (stored securely)",
    password: true,
    ignoreFocusOut: true,
  });
  if (!input) return;
  await secrets.store("remora.token", input.trim());
  vscode.window.showInformationMessage("Remora: Token stored securely");
}

async function cmdSessions(): Promise<void> {
  const config = await ensureConfig();
  if (!config) return;

  let sessions: SessionInfo[];
  try {
    sessions = await fetchSessions(config);
  } catch (err) {
    vscode.window.showErrorMessage(`Remora: Failed to list sessions: ${err}`);
    return;
  }

  if (sessions.length === 0) {
    vscode.window.showInformationMessage("Remora: No sessions found");
    return;
  }

  const items = sessions.map((s) => ({
    label: s.description || s.id.slice(0, 8),
    description: `ID: ${s.id.slice(0, 8)}...`,
    detail: `Created: ${new Date(s.created_at).toLocaleString()}`,
    session: s,
  }));

  const picked = await vscode.window.showQuickPick(items, {
    title: "Remora Sessions",
    placeHolder: "Select a session to join",
  });

  if (picked) {
    chatProvider.joinSession(config, picked.session);
    statusBarItem.text = `$(plug) Remora: ${picked.label}`;
    statusBarItem.tooltip = `Session: ${picked.label}`;
  }
}

async function cmdJoin(): Promise<void> {
  const config = await ensureConfig();
  if (!config) return;

  let sessions: SessionInfo[];
  try {
    sessions = await fetchSessions(config);
  } catch (err) {
    vscode.window.showErrorMessage(`Remora: Failed to list sessions: ${err}`);
    return;
  }

  if (sessions.length === 0) {
    const desc = await vscode.window.showInputBox({
      title: "Create New Session",
      prompt: "No sessions found. Enter a description to create one.",
      placeHolder: "Session description",
      ignoreFocusOut: true,
    });
    if (!desc) return;

    try {
      const newSession = await createSessionApi(config, desc.trim());
      chatProvider.joinSession(config, newSession);
      statusBarItem.text = `$(plug) Remora: ${newSession.description || newSession.id.slice(0, 8)}`;
      vscode.window.showInformationMessage(
        `Remora: Created and joined session "${newSession.description}"`
      );
    } catch (err) {
      vscode.window.showErrorMessage(
        `Remora: Failed to create session: ${err}`
      );
    }
    return;
  }

  const items = sessions.map((s) => ({
    label: s.description || s.id.slice(0, 8),
    description: `ID: ${s.id.slice(0, 8)}...`,
    detail: `Created: ${new Date(s.created_at).toLocaleString()}`,
    session: s,
  }));

  const picked = await vscode.window.showQuickPick(items, {
    title: "Join Session",
    placeHolder: "Select a session to join",
  });

  if (picked) {
    chatProvider.joinSession(config, picked.session);
    statusBarItem.text = `$(plug) Remora: ${picked.label}`;
    statusBarItem.tooltip = `Session: ${picked.label}`;
    vscode.window.showInformationMessage(
      `Remora: Joined session "${picked.label}"`
    );
  }
}

async function cmdLeave(): Promise<void> {
  if (!chatProvider.currentSession) {
    vscode.window.showInformationMessage("Remora: Not in a session");
    return;
  }

  chatProvider.leaveSession();
  statusBarItem.text = "$(plug) Remora: Connected";
  statusBarItem.tooltip = currentConfig
    ? `Connected to ${currentConfig.url} as ${currentConfig.name}`
    : "Remora";
  vscode.window.showInformationMessage("Remora: Left session");
}

async function cmdSend(): Promise<void> {
  if (!chatProvider.isConnected) {
    vscode.window.showWarningMessage("Remora: Not connected to a session");
    return;
  }

  const text = await vscode.window.showInputBox({
    title: "Send Message",
    prompt: "Type a message or /command",
    placeHolder: "Hello team!",
    ignoreFocusOut: true,
  });

  if (text) {
    chatProvider.sendRawText(text);
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Read token from SecretStorage, falling back to plaintext settings for migration. */
async function getToken(): Promise<string> {
  const stored = await secrets.get("remora.token");
  if (stored) return stored;
  // Fall back to legacy plaintext setting
  const config = vscode.workspace.getConfiguration("remora");
  return config.get<string>("token") || "";
}

/** One-time migration: move plaintext token from settings.json to SecretStorage. */
async function migrateTokenToSecrets(): Promise<void> {
  const config = vscode.workspace.getConfiguration("remora");
  const plaintext = config.get<string>("token") || "";
  if (!plaintext) return;
  // Only migrate if SecretStorage doesn't already have a value
  const existing = await secrets.get("remora.token");
  if (existing) return;
  await secrets.store("remora.token", plaintext);
  // Clear the plaintext setting
  await config.update("token", undefined, vscode.ConfigurationTarget.Global);
  await config.update("token", undefined, vscode.ConfigurationTarget.Workspace);
}

async function ensureConfig(): Promise<ConnectionConfig | undefined> {
  if (currentConfig) return currentConfig;
  await cmdConnect();
  return currentConfig;
}

async function checkHealth(baseUrl: string): Promise<boolean> {
  try {
    const https = await import("https");
    const http = await import("http");
    return new Promise((resolve) => {
      const mod = baseUrl.startsWith("https") ? https : http;
      const req = mod.get(`${baseUrl}/health`, (res) => {
        let body = "";
        res.on("data", (chunk: Buffer) => {
          body += chunk.toString();
        });
        res.on("end", () => {
          try {
            const json = JSON.parse(body);
            resolve(json.status === "ok");
          } catch {
            resolve(false);
          }
        });
      });
      req.on("error", () => resolve(false));
      req.setTimeout(5000, () => {
        req.destroy();
        resolve(false);
      });
    });
  } catch {
    return false;
  }
}

async function fetchSessions(
  config: ConnectionConfig
): Promise<SessionInfo[]> {
  const https = await import("https");
  const http = await import("http");
  const url = await import("url");

  return new Promise((resolve, reject) => {
    const parsed = new url.URL(`${config.url}/sessions`);
    const mod = parsed.protocol === "https:" ? https : http;
    const req = mod.get(
      parsed.href,
      {
        headers: { Authorization: `Bearer ${config.token}` },
      },
      (res) => {
        let body = "";
        res.on("data", (chunk: Buffer) => {
          body += chunk.toString();
        });
        res.on("end", () => {
          if (res.statusCode === 401) {
            reject(new Error("Invalid token"));
            return;
          }
          if (!res.statusCode || res.statusCode >= 400) {
            reject(new Error(`Server error: ${res.statusCode}`));
            return;
          }
          try {
            resolve(JSON.parse(body));
          } catch {
            reject(new Error("Invalid JSON response"));
          }
        });
      }
    );
    req.on("error", (err) => reject(err));
    req.setTimeout(10000, () => {
      req.destroy();
      reject(new Error("Request timed out"));
    });
  });
}

async function createSessionApi(
  config: ConnectionConfig,
  description: string
): Promise<SessionInfo> {
  const https = await import("https");
  const http = await import("http");
  const url = await import("url");

  return new Promise((resolve, reject) => {
    const parsed = new url.URL(`${config.url}/sessions`);
    const mod = parsed.protocol === "https:" ? https : http;
    const payload = JSON.stringify({ description });

    const req = mod.request(
      parsed.href,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${config.token}`,
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(payload).toString(),
        },
      },
      (res) => {
        let body = "";
        res.on("data", (chunk: Buffer) => {
          body += chunk.toString();
        });
        res.on("end", () => {
          if (res.statusCode === 401) {
            reject(new Error("Invalid token"));
            return;
          }
          if (!res.statusCode || res.statusCode >= 400) {
            reject(new Error(`Server error: ${res.statusCode}`));
            return;
          }
          try {
            resolve(JSON.parse(body));
          } catch {
            reject(new Error("Invalid JSON response"));
          }
        });
      }
    );
    req.on("error", (err) => reject(err));
    req.setTimeout(10000, () => {
      req.destroy();
      reject(new Error("Request timed out"));
    });
    req.write(payload);
    req.end();
  });
}
