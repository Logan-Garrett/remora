import { describe, it, expect } from "vitest";
import { buildWsUrl } from "../api";

describe("buildWsUrl", () => {
  it("converts http to ws", () => {
    const url = buildWsUrl(
      { url: "http://localhost:7200", token: "abc", name: "bob" },
      "session-123"
    );
    expect(url).toMatch(/^ws:\/\/localhost:7200\/sessions\/session-123\?/);
    expect(url).toContain("token=abc");
    expect(url).toContain("name=bob");
  });

  it("converts https to wss", () => {
    const url = buildWsUrl(
      { url: "https://example.com:7200", token: "tok", name: "me" },
      "sid"
    );
    expect(url).toMatch(/^wss:\/\/example\.com:7200\/sessions\/sid\?/);
  });

  it("encodes special characters in token and name", () => {
    const url = buildWsUrl(
      { url: "http://host:7200", token: "a&b=c", name: "my name" },
      "s1"
    );
    // URLSearchParams encodes & and spaces
    expect(url).not.toContain("a&b=c");
    expect(url).toContain("token=a%26b%3Dc");
  });
});
