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

  it("treats unknown commands as chat", () => {
    expect(parseCommand("/unknown stuff", author)).toEqual({
      type: "chat",
      author: "alice",
      text: "/unknown stuff",
    });
  });
});
