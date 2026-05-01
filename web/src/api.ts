import type { SessionInfo, ConnectionConfig } from "./types";

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

export function buildWsUrl(config: ConnectionConfig, sessionId: string): string {
  const wsBase = config.url.replace(/^http/, "ws");
  const params = new URLSearchParams({
    token: config.token,
    name: config.name,
  });
  return `${wsBase}/sessions/${sessionId}?${params}`;
}
