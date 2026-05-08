import type { ClientMessage, ServerMessage, StreamDelta, RemoraEvent } from "./types";

export type WsEventHandler = (event: RemoraEvent) => void;
export type WsErrorHandler = (message: string) => void;
export type WsCloseHandler = () => void;
export type WsStreamStartHandler = () => void;
export type WsStreamDeltaHandler = (delta: string) => void;
export type WsStreamEndHandler = () => void;

export class RemoraSocket {
  private ws: WebSocket | null = null;
  private onEvent: WsEventHandler;
  private onError: WsErrorHandler;
  private onClose: WsCloseHandler;
  private onStreamStart?: WsStreamStartHandler;
  private onStreamDelta?: WsStreamDeltaHandler;
  private onStreamEnd?: WsStreamEndHandler;
  private url: string;

  constructor(
    url: string,
    handlers: {
      onEvent: WsEventHandler;
      onError: WsErrorHandler;
      onClose: WsCloseHandler;
      onStreamStart?: WsStreamStartHandler;
      onStreamDelta?: WsStreamDeltaHandler;
      onStreamEnd?: WsStreamEndHandler;
    }
  ) {
    this.url = url;
    this.onEvent = handlers.onEvent;
    this.onError = handlers.onError;
    this.onClose = handlers.onClose;
    this.onStreamStart = handlers.onStreamStart;
    this.onStreamDelta = handlers.onStreamDelta;
    this.onStreamEnd = handlers.onStreamEnd;
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

      switch (msg.type) {
        case "error":
          this.onError(msg.message);
          break;
        case "event":
          this.onEvent(msg.data);
          break;
        case "stream_start":
          this.onStreamStart?.();
          break;
        case "stream_delta":
          this.onStreamDelta?.((msg as StreamDelta).delta);
          break;
        case "stream_end":
          this.onStreamEnd?.();
          break;
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
