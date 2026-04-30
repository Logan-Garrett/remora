import type { ClientMessage, ServerMessage, RemoraEvent } from "./types";

export type WsEventHandler = (event: RemoraEvent) => void;
export type WsErrorHandler = (message: string) => void;
export type WsCloseHandler = () => void;

export class RemoraSocket {
  private ws: WebSocket | null = null;
  private onEvent: WsEventHandler;
  private onError: WsErrorHandler;
  private onClose: WsCloseHandler;
  private url: string;

  constructor(
    url: string,
    handlers: {
      onEvent: WsEventHandler;
      onError: WsErrorHandler;
      onClose: WsCloseHandler;
    }
  ) {
    this.url = url;
    this.onEvent = handlers.onEvent;
    this.onError = handlers.onError;
    this.onClose = handlers.onClose;
  }

  connect(): void {
    this.ws = new WebSocket(this.url);

    this.ws.onmessage = (ev: MessageEvent) => {
      let msg: ServerMessage;
      try {
        msg = JSON.parse(ev.data as string) as ServerMessage;
      } catch {
        return;
      }

      if (msg.type === "error") {
        this.onError(msg.message);
      } else if (msg.type === "event") {
        this.onEvent(msg.data);
      }
    };

    this.ws.onclose = () => {
      this.ws = null;
      this.onClose();
    };

    this.ws.onerror = () => {
      this.onError("WebSocket connection failed");
    };
  }

  send(msg: ClientMessage): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    this.ws.send(JSON.stringify(msg));
  }

  close(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  get connected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }
}
