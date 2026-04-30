import type { ClientMessage } from "./types";

/** Parse user input into a ClientMessage. Handles /slash commands and plain chat. */
export function parseCommand(input: string, author: string): ClientMessage | null {
  if (!input) return null;

  if (!input.startsWith("/")) {
    return { type: "chat", author, text: input };
  }

  const parts = input.slice(1).split(/\s+/);
  const cmd = parts[0]?.toLowerCase();
  const rest = parts.slice(1).join(" ");

  switch (cmd) {
    case "run":
      return { type: "run", author };
    case "run-all":
    case "runall":
      return { type: "run_all", author };
    case "clear":
      return { type: "clear", author };
    case "who":
      return { type: "who", author };
    case "info":
    case "session":
      return { type: "session_info", author };
    case "diff":
      return { type: "diff", author };
    case "fetch":
      if (!rest) return { type: "chat", author, text: input };
      return { type: "fetch", author, url: rest };
    default:
      // Unknown command — send as chat so the user sees it
      return { type: "chat", author, text: input };
  }
}
