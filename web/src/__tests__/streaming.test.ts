import { describe, it, expect, beforeEach } from "vitest";

/**
 * Tests for token-level streaming support.
 *
 * These tests verify that the streaming callbacks behave correctly
 * when processing stream_start, stream_delta, and stream_end messages
 * without requiring a real WebSocket connection.
 */

describe("streaming state management", () => {
  let streamBuffer: string;
  let streamingEl: { textContent: string } | null;

  // Mirrors the callback logic in chat.ts
  function onStreamStart(): void {
    streamBuffer = "";
    streamingEl = { textContent: "" };
  }

  function onStreamDelta(delta: string): void {
    streamBuffer += delta;
    if (streamingEl) {
      streamingEl.textContent = streamBuffer;
    }
  }

  function onStreamEnd(): void {
    streamingEl = null;
    streamBuffer = "";
  }

  beforeEach(() => {
    streamBuffer = "";
    streamingEl = null;
  });

  it("StreamStart initializes a fresh buffer", () => {
    // Simulate leftover state from a previous stream
    streamBuffer = "leftover text";
    streamingEl = { textContent: "old content" };

    onStreamStart();

    expect(streamBuffer).toBe("");
    expect(streamingEl).not.toBeNull();
    expect(streamingEl!.textContent).toBe("");
  });

  it("StreamDelta messages accumulate text correctly", () => {
    onStreamStart();

    onStreamDelta("Hello");
    expect(streamBuffer).toBe("Hello");
    expect(streamingEl!.textContent).toBe("Hello");

    onStreamDelta(", ");
    expect(streamBuffer).toBe("Hello, ");
    expect(streamingEl!.textContent).toBe("Hello, ");

    onStreamDelta("world!");
    expect(streamBuffer).toBe("Hello, world!");
    expect(streamingEl!.textContent).toBe("Hello, world!");
  });

  it("StreamEnd resets state", () => {
    onStreamStart();
    onStreamDelta("some content");

    expect(streamBuffer).toBe("some content");
    expect(streamingEl).not.toBeNull();

    onStreamEnd();

    expect(streamBuffer).toBe("");
    expect(streamingEl).toBeNull();
  });

  it("StreamDelta without StreamStart does not throw", () => {
    // streamingEl is null, delta should still accumulate in buffer
    expect(() => onStreamDelta("orphan delta")).not.toThrow();
    expect(streamBuffer).toBe("orphan delta");
  });

  it("multiple stream cycles work independently", () => {
    // First cycle
    onStreamStart();
    onStreamDelta("first ");
    onStreamDelta("response");
    expect(streamBuffer).toBe("first response");
    onStreamEnd();

    // Second cycle
    onStreamStart();
    expect(streamBuffer).toBe("");
    onStreamDelta("second response");
    expect(streamBuffer).toBe("second response");
    expect(streamingEl!.textContent).toBe("second response");
    onStreamEnd();

    expect(streamBuffer).toBe("");
    expect(streamingEl).toBeNull();
  });
});
