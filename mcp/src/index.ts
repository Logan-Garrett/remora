#!/usr/bin/env node
/**
 * Remora MCP Server
 *
 * Exposes a Remora collaborative Claude session as MCP tools.
 * Maintains a persistent WebSocket connection and buffers events.
 *
 * Tools:
 *   remora_health      — check server health
 *   remora_sessions    — list available sessions
 *   remora_create      — create a new session
 *   remora_join        — join a session (opens persistent WebSocket)
 *   remora_send        — send a chat message to the current session
 *   remora_run         — trigger /run (invoke Claude in the session)
 *   remora_events      — read buffered events
 *   remora_leave       — disconnect from the current session
 *   remora_delete      — delete a session
 *
 * Environment:
 *   REMORA_URL         — server URL (default: http://localhost:7200)
 *   REMORA_TEAM_TOKEN  — auth token (required)
 *   REMORA_NAME        — display name (default: claude-mcp)
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import WebSocket from "ws";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

const SERVER_URL = process.env.REMORA_URL ?? "http://localhost:7200";
const TOKEN = process.env.REMORA_TEAM_TOKEN ?? "";
const DEFAULT_NAME = process.env.REMORA_NAME ?? "claude-mcp";

interface RemoraEvent {
  type: string;
  data?: {
    id: number;
    session_id: string;
    timestamp: string;
    author: string | null;
    kind: string;
    payload: Record<string, unknown>;
  };
  message?: string;
}

// ── State ────────────────────────────────────────────────────────────────────

let ws: WebSocket | null = null;
let currentSession: string | null = null;
let currentName: string = DEFAULT_NAME;
let eventBuffer: RemoraEvent[] = [];
const MAX_BUFFER = 200;

function bufferEvent(event: RemoraEvent): void {
  eventBuffer.push(event);
  if (eventBuffer.length > MAX_BUFFER) {
    eventBuffer = eventBuffer.slice(-MAX_BUFFER);
  }
}

// ── HTTP helpers ─────────────────────────────────────────────────────────────

async function api(
  method: string,
  path: string,
  body?: unknown
): Promise<unknown> {
  const url = `${SERVER_URL}${path}`;
  const headers: Record<string, string> = {
    Authorization: `Bearer ${TOKEN}`,
  };
  if (body) headers["Content-Type"] = "application/json";

  const res = await fetch(url, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text}`);
  }

  const ct = res.headers.get("content-type") ?? "";
  if (ct.includes("json")) return res.json();
  return res.text();
}

// ── WebSocket ────────────────────────────────────────────────────────────────

function connect(sessionId: string, name: string): Promise<void> {
  return new Promise((resolve, reject) => {
    if (ws) {
      ws.close();
      ws = null;
    }

    const wsUrl = SERVER_URL.replace(/^http/, "ws");
    const fullUrl = `${wsUrl}/sessions/${sessionId}?token=${TOKEN}&name=${encodeURIComponent(name)}`;

    const socket = new WebSocket(fullUrl);

    socket.on("open", () => {
      ws = socket;
      currentSession = sessionId;
      currentName = name;
      eventBuffer = [];
      resolve();
    });

    socket.on("message", (data) => {
      try {
        const event = JSON.parse(data.toString()) as RemoraEvent;
        bufferEvent(event);
      } catch {
        // ignore malformed messages
      }
    });

    socket.on("close", () => {
      ws = null;
      currentSession = null;
    });

    socket.on("error", (err) => {
      reject(new Error(`WebSocket error: ${err.message}`));
    });

    setTimeout(() => {
      if (!ws) reject(new Error("WebSocket connection timeout"));
    }, 10000);
  });
}

function disconnect(): void {
  if (ws) {
    ws.close();
    ws = null;
    currentSession = null;
  }
}

function send(msg: Record<string, unknown>): void {
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    throw new Error("Not connected to a session. Use remora_join first.");
  }
  ws.send(JSON.stringify(msg));
}

// ── MCP Server ───────────────────────────────────────────────────────────────

const server = new McpServer({
  name: "remora",
  version: "0.9.2",
});

// Health check
server.tool("remora_health", "Check if the Remora server is reachable", {}, async () => {
  const result = await api("GET", "/health");
  return { content: [{ type: "text", text: JSON.stringify(result) }] };
});

// List sessions
server.tool("remora_sessions", "List all available Remora sessions", {}, async () => {
  const sessions = await api("GET", "/sessions");
  return { content: [{ type: "text", text: JSON.stringify(sessions, null, 2) }] };
});

// Create session
server.tool(
  "remora_create",
  "Create a new Remora session",
  { description: z.string().describe("Session description") },
  async ({ description }) => {
    const result = await api("POST", "/sessions", { description, repos: [] });
    return { content: [{ type: "text", text: JSON.stringify(result, null, 2) }] };
  }
);

// Join session
server.tool(
  "remora_join",
  "Join a Remora session (opens persistent WebSocket connection)",
  {
    session_id: z.string().describe("UUID of the session to join"),
    name: z.string().optional().describe("Display name (default: claude-mcp)"),
  },
  async ({ session_id, name }) => {
    const displayName = name ?? DEFAULT_NAME;
    await connect(session_id, displayName);
    return {
      content: [
        {
          type: "text",
          text: `Connected to session ${session_id} as "${displayName}". Use remora_send to chat, remora_run to invoke Claude, remora_events to read messages.`,
        },
      ],
    };
  }
);

// Send message
server.tool(
  "remora_send",
  "Send a chat message to the current Remora session",
  { text: z.string().describe("Message text to send") },
  async ({ text }) => {
    send({ type: "chat", author: currentName, text });
    return { content: [{ type: "text", text: `Sent: "${text}"` }] };
  }
);

// Trigger /run
server.tool(
  "remora_run",
  "Trigger /run in the current session — invokes Claude with recent context",
  {},
  async () => {
    send({ type: "run", author: currentName });
    return {
      content: [
        {
          type: "text",
          text: "Claude run triggered. Use remora_events to read the response once it arrives.",
        },
      ],
    };
  }
);

// Read events
server.tool(
  "remora_events",
  "Read buffered events from the current session (most recent first)",
  {
    count: z.number().optional().describe("Number of events to return (default: 20)"),
    kind: z.string().optional().describe("Filter by event kind (chat, assistant_response, tool_use, system)"),
  },
  async ({ count, kind }) => {
    const n = count ?? 20;
    let events = eventBuffer.slice(-n);
    if (kind) {
      events = events.filter((e) => e.data?.kind === kind);
    }
    const formatted = events.map((e) => {
      if (e.type === "error") return `[ERROR] ${e.message}`;
      const d = e.data;
      if (!d) return JSON.stringify(e);
      const author = d.author ? `${d.author}: ` : "";
      const text = (d.payload.text as string) ?? JSON.stringify(d.payload);
      return `[${d.kind}] ${author}${text}`;
    });
    return {
      content: [
        {
          type: "text",
          text: formatted.length > 0
            ? formatted.join("\n")
            : "(no events yet — messages will appear here as they arrive)",
        },
      ],
    };
  }
);

// Leave session
server.tool("remora_leave", "Disconnect from the current Remora session", {}, async () => {
  const sid = currentSession;
  disconnect();
  return { content: [{ type: "text", text: `Disconnected from session ${sid}` }] };
});

// Delete session
server.tool(
  "remora_delete",
  "Delete a Remora session",
  { session_id: z.string().describe("UUID of the session to delete") },
  async ({ session_id }) => {
    await api("DELETE", `/sessions/${session_id}`);
    return { content: [{ type: "text", text: `Session ${session_id} deleted` }] };
  }
);

// ── Prompt Templates ─────────────────────────────────────────────────────────

interface Template {
  name: string;
  description: string;
  content: string;
}

function loadTemplates(): Template[] {
  // Look for templates relative to the MCP server, then in the repo root
  const searchPaths = [
    resolve(import.meta.dirname ?? ".", "../../templates"),
    resolve(import.meta.dirname ?? ".", "../../../templates"),
    process.env.REMORA_TEMPLATES_DIR ?? "",
  ].filter(Boolean);

  for (const dir of searchPaths) {
    if (!existsSync(dir)) continue;
    const files = readdirSync(dir).filter((f) => f.endsWith(".md"));
    return files.map((f) => {
      const raw = readFileSync(join(dir, f), "utf-8");
      const frontmatterMatch = raw.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
      if (!frontmatterMatch) {
        return { name: f.replace(".md", ""), description: "", content: raw.trim() };
      }
      const meta = frontmatterMatch[1];
      const content = frontmatterMatch[2].trim();
      const name = meta.match(/name:\s*(.+)/)?.[1]?.trim() ?? f.replace(".md", "");
      const description = meta.match(/description:\s*(.+)/)?.[1]?.trim() ?? "";
      return { name, description, content };
    });
  }
  return [];
}

const templates = loadTemplates();

// Register each template as an MCP prompt
for (const tmpl of templates) {
  server.prompt(
    tmpl.name,
    tmpl.description,
    {},
    async () => ({
      messages: [
        {
          role: "user" as const,
          content: { type: "text" as const, text: tmpl.content },
        },
      ],
    })
  );
}

// Tool to list available templates (for clients that don't support MCP prompts natively)
server.tool(
  "remora_templates",
  "List available prompt templates",
  {},
  async () => {
    const list = templates.map((t) => `- **${t.name}** — ${t.description}`).join("\n");
    return {
      content: [
        {
          type: "text",
          text: templates.length > 0
            ? `Available templates:\n${list}\n\nUse remora_template_run to execute one.`
            : "No templates found. Add .md files to the templates/ directory.",
        },
      ],
    };
  }
);

// Tool to run a template (sends its content as a chat message + triggers /run)
server.tool(
  "remora_template_run",
  "Run a prompt template — sends the template text to the session and triggers /run",
  { name: z.string().describe("Template name (e.g. 'summarize', 'review', 'pr-description')") },
  async ({ name }) => {
    const tmpl = templates.find((t) => t.name === name);
    if (!tmpl) {
      return {
        content: [{ type: "text", text: `Template "${name}" not found. Use remora_templates to list available templates.` }],
      };
    }
    send({ type: "chat", author: currentName, text: `[template: ${tmpl.name}]\n${tmpl.content}` });
    send({ type: "run", author: currentName });
    return {
      content: [
        {
          type: "text",
          text: `Template "${tmpl.name}" sent to session and /run triggered. Use remora_events to read the response.`,
        },
      ],
    };
  }
);

// ── Start ────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch((err) => {
  console.error("Failed to start MCP server:", err);
  process.exit(1);
});
