import { sendNotification } from "@tauri-apps/api/notification";

// Listen for Claude responses and show native notifications
// when the window is not focused
document.addEventListener("remora:claude_response", (e: CustomEvent) => {
  if (!document.hasFocus()) {
    sendNotification({
      title: "Remora",
      body: "Claude finished responding"
    });
  }
});

// Deep link handling
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
onOpenUrl((urls) => {
  for (const url of urls) {
    // Parse remora://join/<server-url>/<session-id>/<token>
    const match = url.match(/^remora:\/\/join\/(.+)\/([a-f0-9-]+)\/(.+)$/);
    if (match) {
      const [, serverUrl, sessionId, token] = match;
      // Store in sessionStorage and navigate to chat
      sessionStorage.setItem("remora_config", JSON.stringify({
        url: decodeURIComponent(serverUrl),
        token: decodeURIComponent(token),
        name: "desktop-user"
      }));
      window.dispatchEvent(new CustomEvent("remora:join", { detail: { sessionId } }));
    }
  }
});
