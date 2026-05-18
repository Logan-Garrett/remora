import { sendNotification } from "@tauri-apps/api/notification";

// Listen for Claude responses and show native notifications
// when the window is not focused
document.addEventListener("remora:claude_response", () => {
  if (!document.hasFocus()) {
    sendNotification({
      title: "Remora",
      body: "Claude finished responding"
    });
  }
});

// Deep link handling — requires user confirmation to prevent a malicious app
// on the same machine from silently redirecting the user to an attacker's server.
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
onOpenUrl((urls) => {
  for (const raw of urls) {
    // Parse remora://join/<server-url>/<session-id>/<token>
    const match = raw.match(/^remora:\/\/join\/([^/]+)\/([a-f0-9A-F-]+)\/([^/]+)$/);
    if (!match) continue;

    const serverUrl = decodeURIComponent(match[1]);
    const sessionId = match[2];

    // Validate server URL is HTTP(S)
    if (!serverUrl.startsWith("http://") && !serverUrl.startsWith("https://")) continue;

    // Prompt user before connecting — never auto-connect from deep links
    const confirmed = window.confirm(
      `A deep link is asking to connect to:\n\n` +
      `Server: ${serverUrl}\n` +
      `Session: ${sessionId.slice(0, 8)}...\n\n` +
      `Do you want to connect?`
    );
    if (!confirmed) continue;

    const token = decodeURIComponent(match[3]);
    sessionStorage.setItem("remora_config", JSON.stringify({
      url: serverUrl,
      token: token,
      name: "desktop-user"
    }));
    window.dispatchEvent(new CustomEvent("remora:join", { detail: { sessionId } }));
  }
});
