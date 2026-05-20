import type {
  SessionInfo,
  ConnectionConfig,
  AuthResponse,
  UsageResponse,
  RunAnalytics,
  AdminSessionInfo,
  AdminUser,
  AuditEvent,
} from "./types";

export async function fetchHealth(baseUrl: string): Promise<boolean> {
  try {
    const resp = await fetch(`${baseUrl}/health`);
    if (!resp.ok) return false;
    const body = await resp.json();
    return body.status === "ok";
  } catch {
    return false;
  }
}

export async function listSessions(
  config: ConnectionConfig
): Promise<SessionInfo[]> {
  const resp = await fetch(`${config.url}/sessions`, {
    headers: { Authorization: `Bearer ${config.token}` },
  });
  if (resp.status === 401) throw new Error("Invalid token");
  if (!resp.ok) throw new Error(`Server error: ${resp.status}`);
  return resp.json();
}

export async function createSession(
  config: ConnectionConfig,
  description: string,
  repos?: string[]
): Promise<SessionInfo> {
  const body: Record<string, unknown> = { description };
  if (repos && repos.length > 0) {
    body.repos = repos;
  }
  const resp = await fetch(`${config.url}/sessions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${config.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (resp.status === 401) throw new Error("Invalid token");
  if (!resp.ok) throw new Error(`Server error: ${resp.status}`);
  return resp.json();
}

export async function deleteSession(
  config: ConnectionConfig,
  sessionId: string
): Promise<void> {
  const resp = await fetch(`${config.url}/sessions/${sessionId}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${config.token}` },
  });
  if (resp.status === 401) throw new Error("Invalid token");
  if (!resp.ok && resp.status !== 204)
    throw new Error(`Server error: ${resp.status}`);
}

export async function reactivateSession(
  config: ConnectionConfig,
  sessionId: string
): Promise<void> {
  const resp = await fetch(`${config.url}/sessions/${sessionId}/reactivate`, {
    method: "POST",
    headers: { Authorization: `Bearer ${config.token}` },
  });
  if (resp.status === 401) throw new Error("Invalid token");
  if (!resp.ok && resp.status !== 204)
    throw new Error(`Server error: ${resp.status}`);
}

const OWNER_KEY_PREFIX = "remora_owner_key_";

/** Store an owner_key for a session (called after creating a session). */
export function storeOwnerKey(sessionId: string, ownerKey: string): void {
  sessionStorage.setItem(`${OWNER_KEY_PREFIX}${sessionId}`, ownerKey);
}

/** Retrieve a stored owner_key for a session, if any. */
export function getOwnerKey(sessionId: string): string | null {
  return sessionStorage.getItem(`${OWNER_KEY_PREFIX}${sessionId}`);
}

export async function authRegister(
  baseUrl: string,
  email: string,
  displayName: string,
  password: string
): Promise<void> {
  const resp = await fetch(`${baseUrl}/auth/register`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, display_name: displayName, password }),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `Registration failed (${resp.status})`);
  }
}

export async function authLogin(
  baseUrl: string,
  email: string,
  password: string
): Promise<AuthResponse> {
  const resp = await fetch(`${baseUrl}/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `Login failed (${resp.status})`);
  }
  return resp.json();
}

// ── Admin API ────────────────────────────────────────────────────────────────

function adminHeaders(config: ConnectionConfig): Record<string, string> {
  return { Authorization: `Bearer ${config.token}` };
}

export async function adminGetUsage(config: ConnectionConfig): Promise<UsageResponse> {
  const resp = await fetch(`${config.url}/admin/usage`, { headers: adminHeaders(config) });
  if (!resp.ok) throw new Error(`Admin usage: ${resp.status}`);
  return resp.json();
}

export async function adminGetAnalytics(config: ConnectionConfig): Promise<RunAnalytics> {
  const resp = await fetch(`${config.url}/admin/analytics`, { headers: adminHeaders(config) });
  if (!resp.ok) throw new Error(`Admin analytics: ${resp.status}`);
  return resp.json();
}

export async function adminListSessions(config: ConnectionConfig): Promise<AdminSessionInfo[]> {
  const resp = await fetch(`${config.url}/admin/sessions`, { headers: adminHeaders(config) });
  if (!resp.ok) throw new Error(`Admin sessions: ${resp.status}`);
  return resp.json();
}

export async function adminUpdateQuota(
  config: ConnectionConfig,
  sessionId: string,
  dailyTokenCap: number
): Promise<void> {
  const resp = await fetch(`${config.url}/admin/sessions/${sessionId}/quota`, {
    method: "PUT",
    headers: { ...adminHeaders(config), "Content-Type": "application/json" },
    body: JSON.stringify({ daily_token_cap: dailyTokenCap }),
  });
  if (!resp.ok) throw new Error(`Update quota: ${resp.status}`);
}

export async function adminDeleteSession(
  config: ConnectionConfig,
  sessionId: string
): Promise<void> {
  const resp = await fetch(`${config.url}/admin/sessions/${sessionId}`, {
    method: "DELETE",
    headers: adminHeaders(config),
  });
  if (!resp.ok && resp.status !== 204) throw new Error(`Delete session: ${resp.status}`);
}

export async function adminExpireSession(
  config: ConnectionConfig,
  sessionId: string
): Promise<void> {
  const resp = await fetch(`${config.url}/admin/sessions/${sessionId}/expire`, {
    method: "POST",
    headers: adminHeaders(config),
  });
  if (!resp.ok) throw new Error(`Expire session: ${resp.status}`);
}

export async function adminListUsers(config: ConnectionConfig): Promise<AdminUser[]> {
  const resp = await fetch(`${config.url}/admin/users`, { headers: adminHeaders(config) });
  if (!resp.ok) throw new Error(`Admin users: ${resp.status}`);
  return resp.json();
}

export async function adminUpdateUserRole(
  config: ConnectionConfig,
  userId: string,
  role: string
): Promise<void> {
  const resp = await fetch(`${config.url}/admin/users/${userId}/role`, {
    method: "PUT",
    headers: { ...adminHeaders(config), "Content-Type": "application/json" },
    body: JSON.stringify({ role }),
  });
  if (!resp.ok) throw new Error(`Update role: ${resp.status}`);
}

export async function adminListAuditEvents(
  config: ConnectionConfig,
  limit = 50,
  offset = 0
): Promise<AuditEvent[]> {
  const resp = await fetch(
    `${config.url}/admin/audit?limit=${limit}&offset=${offset}`,
    { headers: adminHeaders(config) }
  );
  if (!resp.ok) throw new Error(`Audit events: ${resp.status}`);
  return resp.json();
}

export function buildWsUrl(config: ConnectionConfig, sessionId: string): string {
  const wsBase = config.url.replace(/^http/, "ws");
  const params = new URLSearchParams({
    token: config.token,
    name: config.name,
  });
  // Attach owner_key if we have one for this session
  const ownerKey = getOwnerKey(sessionId);
  if (ownerKey) {
    params.set("owner_key", ownerKey);
  }
  return `${wsBase}/sessions/${sessionId}?${params}`;
}
