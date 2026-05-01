import { describe, it, expect } from "vitest";
import { parseCommand } from "../commands";

describe("parseCommand", () => {
  const author = "alice";

  it("returns null for empty input", () => {
    expect(parseCommand("", author)).toBeNull();
  });

  it("treats plain text as chat", () => {
    expect(parseCommand("hello world", author)).toEqual({
      type: "chat",
      author: "alice",
      text: "hello world",
    });
  });

  it("parses /run", () => {
    expect(parseCommand("/run", author)).toEqual({
      type: "run",
      author: "alice",
    });
  });

  it("parses /run-all", () => {
    expect(parseCommand("/run-all", author)).toEqual({
      type: "run_all",
      author: "alice",
    });
  });

  it("parses /runall", () => {
    expect(parseCommand("/runall", author)).toEqual({
      type: "run_all",
      author: "alice",
    });
  });

  it("parses /clear", () => {
    expect(parseCommand("/clear", author)).toEqual({
      type: "clear",
      author: "alice",
    });
  });

  it("parses /who", () => {
    expect(parseCommand("/who", author)).toEqual({
      type: "who",
      author: "alice",
    });
  });

  it("parses /info", () => {
    expect(parseCommand("/info", author)).toEqual({
      type: "session_info",
      author: "alice",
    });
  });

  it("parses /session as session_info", () => {
    expect(parseCommand("/session", author)).toEqual({
      type: "session_info",
      author: "alice",
    });
  });

  it("parses /diff", () => {
    expect(parseCommand("/diff", author)).toEqual({
      type: "diff",
      author: "alice",
    });
  });

  it("parses /fetch with url", () => {
    expect(parseCommand("/fetch https://example.com", author)).toEqual({
      type: "fetch",
      author: "alice",
      url: "https://example.com",
    });
  });

  it("treats /fetch without url as chat", () => {
    expect(parseCommand("/fetch", author)).toEqual({
      type: "chat",
      author: "alice",
      text: "/fetch",
    });
  });

  it("parses /add with path", () => {
    expect(parseCommand("/add src/main.rs", author)).toEqual({
      type: "add",
      author: "alice",
      path: "src/main.rs",
    });
  });

  it("treats /add without path as chat", () => {
    expect(parseCommand("/add", author)).toEqual({
      type: "chat",
      author: "alice",
      text: "/add",
    });
  });

  it("parses /repo list", () => {
    expect(parseCommand("/repo list", author)).toEqual({
      type: "repo_list",
      author: "alice",
    });
  });

  it("parses /repo add with url", () => {
    expect(
      parseCommand("/repo add https://github.com/user/repo.git", author)
    ).toEqual({
      type: "repo_add",
      author: "alice",
      git_url: "https://github.com/user/repo.git",
    });
  });

  it("parses /repo remove with name", () => {
    expect(parseCommand("/repo remove myrepo", author)).toEqual({
      type: "repo_remove",
      author: "alice",
      name: "myrepo",
    });
  });

  it("parses /repo rm as remove", () => {
    expect(parseCommand("/repo rm myrepo", author)).toEqual({
      type: "repo_remove",
      author: "alice",
      name: "myrepo",
    });
  });

  it("/repo with no subcommand defaults to list", () => {
    expect(parseCommand("/repo", author)).toEqual({
      type: "repo_list",
      author: "alice",
    });
  });

  it("parses /allowlist (show)", () => {
    expect(parseCommand("/allowlist", author)).toEqual({
      type: "allowlist",
      author: "alice",
    });
  });

  it("parses /allowlist add", () => {
    expect(parseCommand("/allowlist add example.com", author)).toEqual({
      type: "allowlist_add",
      author: "alice",
      domain: "example.com",
    });
  });

  it("parses /allowlist remove", () => {
    expect(parseCommand("/allowlist remove example.com", author)).toEqual({
      type: "allowlist_remove",
      author: "alice",
      domain: "example.com",
    });
  });

  it("parses /approve", () => {
    expect(parseCommand("/approve example.com", author)).toEqual({
      type: "approve",
      author: "alice",
      domain: "example.com",
      approved: true,
    });
  });

  it("parses /deny", () => {
    expect(parseCommand("/deny example.com", author)).toEqual({
      type: "approve",
      author: "alice",
      domain: "example.com",
      approved: false,
    });
  });

  it("parses /kick", () => {
    expect(parseCommand("/kick bob", author)).toEqual({
      type: "kick",
      author: "alice",
      target: "bob",
    });
  });

  it("treats unknown commands as chat", () => {
    expect(parseCommand("/unknown stuff", author)).toEqual({
      type: "chat",
      author: "alice",
      text: "/unknown stuff",
    });
  });
});
