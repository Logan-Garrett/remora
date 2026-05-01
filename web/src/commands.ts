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
    case "help":
    case "?":
      return { type: "help", author };
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
    case "add":
      if (!rest) return { type: "chat", author, text: input };
      return { type: "add", author, path: rest };
    case "repo":
      return parseRepoSubcommand(parts.slice(1), author, input);
    case "allowlist":
      return parseAllowlistSubcommand(parts.slice(1), author, input);
    case "approve":
      if (!rest) return { type: "chat", author, text: input };
      return { type: "approve", author, domain: rest, approved: true };
    case "deny":
      if (!rest) return { type: "chat", author, text: input };
      return { type: "approve", author, domain: rest, approved: false };
    case "kick":
      if (!rest) return { type: "chat", author, text: input };
      return { type: "kick", author, target: rest };
    default:
      return { type: "chat", author, text: input };
  }
}

function parseRepoSubcommand(
  args: string[],
  author: string,
  original: string
): ClientMessage {
  const sub = args[0]?.toLowerCase();
  const value = args.slice(1).join(" ");

  switch (sub) {
    case "add":
      if (!value) return { type: "chat", author, text: original };
      return { type: "repo_add", author, git_url: value };
    case "remove":
    case "rm":
      if (!value) return { type: "chat", author, text: original };
      return { type: "repo_remove", author, name: value };
    case "list":
    case "ls":
      return { type: "repo_list", author };
    default:
      return { type: "repo_list", author };
  }
}

function parseAllowlistSubcommand(
  args: string[],
  author: string,
  original: string
): ClientMessage {
  const sub = args[0]?.toLowerCase();
  const value = args.slice(1).join(" ");

  switch (sub) {
    case "add":
      if (!value) return { type: "chat", author, text: original };
      return { type: "allowlist_add", author, domain: value };
    case "remove":
    case "rm":
      if (!value) return { type: "chat", author, text: original };
      return { type: "allowlist_remove", author, domain: value };
    default:
      return { type: "allowlist", author };
  }
}
