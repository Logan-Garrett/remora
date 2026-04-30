import { renderLogin, clearConfig } from "./login";
import { renderSessions } from "./sessions";
import { renderChat } from "./chat";
import type { ConnectionConfig, SessionInfo } from "./types";

const app = document.getElementById("app")!;

function showLogin(): void {
  renderLogin(app, (config: ConnectionConfig) => {
    showSessions(config);
  });
}

function showSessions(config: ConnectionConfig): void {
  renderSessions(
    app,
    config,
    (session: SessionInfo) => {
      showChat(config, session);
    },
    () => {
      clearConfig();
      showLogin();
    }
  );
}

function showChat(config: ConnectionConfig, session: SessionInfo): void {
  renderChat(app, config, session, () => {
    showSessions(config);
  });
}

showLogin();
