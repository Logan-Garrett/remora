export interface SessionInfo {
  id: string;
  description: string;
  created_at: string;
  status: string;
  owner_key?: string;
}

export interface RemoraEvent {
  id: number;
  session_id: string;
  timestamp: string;
  author: string | null;
  kind: string;
  payload: Record<string, unknown>;
}

export interface ServerEvent {
  type: "event";
  data: RemoraEvent;
}

export interface ServerError {
  type: "error";
  message: string;
}

export interface StreamStart {
  type: "stream_start";
  session_id: string;
}

export interface StreamDelta {
  type: "stream_delta";
  session_id: string;
  delta: string;
}

export interface StreamEnd {
  type: "stream_end";
  session_id: string;
}

export type ServerMessage = ServerEvent | ServerError | StreamStart | StreamDelta | StreamEnd;

export interface ClientChat {
  type: "chat";
  author: string;
  text: string;
}

export interface ClientRun {
  type: "run" | "run_all";
  author: string;
}

export interface ClientHelp {
  type: "help";
  author: string;
}

export interface ClientClear {
  type: "clear";
  author: string;
}

export interface ClientWho {
  type: "who";
  author: string;
}

export interface ClientSessionInfo {
  type: "session_info";
  author: string;
}

export interface ClientDiff {
  type: "diff";
  author: string;
}

export interface ClientFetch {
  type: "fetch";
  author: string;
  url: string;
}

export interface ClientAdd {
  type: "add";
  author: string;
  path: string;
}

export interface ClientRepoAdd {
  type: "repo_add";
  author: string;
  git_url: string;
}

export interface ClientRepoRemove {
  type: "repo_remove";
  author: string;
  name: string;
}

export interface ClientRepoList {
  type: "repo_list";
  author: string;
}

export interface ClientAllowlist {
  type: "allowlist";
  author: string;
}

export interface ClientAllowlistAdd {
  type: "allowlist_add";
  author: string;
  domain: string;
}

export interface ClientAllowlistRemove {
  type: "allowlist_remove";
  author: string;
  domain: string;
}

export interface ClientApprove {
  type: "approve";
  author: string;
  domain: string;
  approved: boolean;
}

export interface ClientKick {
  type: "kick";
  author: string;
  target: string;
}

export interface ClientTrust {
  type: "trust";
  author: string;
  target: string;
}

export interface ClientUntrust {
  type: "untrust";
  author: string;
  target: string;
}

export type ClientMessage =
  | ClientHelp
  | ClientChat
  | ClientRun
  | ClientClear
  | ClientWho
  | ClientSessionInfo
  | ClientDiff
  | ClientFetch
  | ClientAdd
  | ClientRepoAdd
  | ClientRepoRemove
  | ClientRepoList
  | ClientAllowlist
  | ClientAllowlistAdd
  | ClientAllowlistRemove
  | ClientApprove
  | ClientKick
  | ClientTrust
  | ClientUntrust;

export interface ConnectionConfig {
  url: string;
  token: string;
  name: string;
  isAdmin?: boolean;
}

// ── Admin types ──────────────────────────────────────────────────────────────

export interface SessionUsage {
  session_id: string;
  description: string;
  tokens_used_today: number;
  daily_token_cap: number;
  tokens_reset_date: string;
}

export interface GlobalUsage {
  total_tokens_today: number;
  total_sessions: number;
  active_sessions: number;
}

export interface UsageResponse {
  sessions: SessionUsage[];
  global: GlobalUsage;
}

export interface SessionRunCount {
  session_id: string;
  run_count: number;
}

export interface RunAnalytics {
  total_runs: number;
  successful: number;
  failed: number;
  timed_out: number;
  avg_duration_secs: number;
  runs_by_session: SessionRunCount[];
}

export interface AdminSessionInfo {
  id: string;
  description: string;
  created_at: string;
  status: string;
  tokens_used_today: number;
  daily_token_cap: number;
}

export interface AuditEvent {
  id: number;
  user_id: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  details: Record<string, unknown> | null;
  ip_address: string | null;
  created_at: string;
}

export interface AdminUser {
  id: string;
  email: string;
  display_name: string;
  role: string;
  created_at: string;
}

export interface AuthUser {
  id: string;
  email: string;
  display_name: string;
  role: string;
  created_at: string;
}

export interface AuthResponse {
  access_token: string;
  refresh_token: string;
  user: AuthUser;
}
