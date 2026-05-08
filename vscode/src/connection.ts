import WebSocket from "ws";
import type { ServerMessage, StreamDelta } from "./types";

export type ConnectionStatus = "connected" | "connecting" | "disconnected";

export class RemoraConnection {
  private ws: WebSocket | null = null;
  private reconnectAttempts = 0;
  private maxReconnects = 3;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private url: string,
    private token: string,
    private sessionId: string,
    private name: string,
    private onMessage: (data: ServerMessage) => void,
    private onStatus: (status: ConnectionStatus) => void
  ) {}

  connect(): void {
    this.onStatus("connecting");

    const wsUrl =
      this.url.replace(/^http/, "ws") +
      `/sessions/${this.sessionId}?token=${encodeURIComponent(this.token)}&name=${encodeURIComponent(this.name)}`;

    this.ws = new WebSocket(wsUrl);

    this.ws.on("open", () => {
      this.reconnectAttempts = 0;
      this.onStatus("connected");
    });

    this.ws.on("message", (data: WebSocket.Data) => {
      try {
        const msg = JSON.parse(data.toString()) as ServerMessage;
        this.onMessage(msg);
      } catch {
        // ignore malformed messages
      }
    });

    this.ws.on("close", () => {
      this.ws = null;
      this.onStatus("disconnected");
      if (this.reconnectAttempts < this.maxReconnects) {
        this.reconnectAttempts++;
        this.reconnectTimer = setTimeout(() => this.connect(), 2000);
      }
    });

    this.ws.on("error", () => {
      // error is always followed by close, so no action needed here
    });
  }

  send(msg: object): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  disconnect(): void {
    this.maxReconnects = 0; // prevent reconnect
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
    this.ws?.close();
    this.ws = null;
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }
}
