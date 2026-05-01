export interface SessionInfo {
  id: string;
  description: string;
  created_at: string;
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

export type ServerMessage = ServerEvent | ServerError;

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
}
