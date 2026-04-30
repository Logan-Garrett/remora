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

export type ClientMessage =
  | ClientChat
  | ClientRun
  | ClientClear
  | ClientWho
  | ClientSessionInfo
  | ClientDiff
  | ClientFetch;

export interface ConnectionConfig {
  url: string;
  token: string;
  name: string;
}
