import { describe, it, expect, beforeEach } from "vitest";
import { el, clear } from "../dom";

describe("el", () => {
  it("creates an element with tag", () => {
    const div = el("div");
    expect(div.tagName).toBe("DIV");
  });

  it("sets class attribute", () => {
    const div = el("div", { class: "foo bar" });
    expect(div.className).toBe("foo bar");
  });

  it("sets other attributes", () => {
    const input = el("input", { type: "text", placeholder: "hi" });
    expect(input.getAttribute("type")).toBe("text");
    expect(input.getAttribute("placeholder")).toBe("hi");
  });

  it("appends string children as text nodes (not HTML)", () => {
    const div = el("div", {}, '<script>alert("xss")</script>');
    // Must be a text node, not parsed as HTML
    expect(div.innerHTML).toBe(
      '&lt;script&gt;alert("xss")&lt;/script&gt;'
    );
    expect(div.childNodes.length).toBe(1);
    expect(div.childNodes[0].nodeType).toBe(Node.TEXT_NODE);
  });

  it("appends element children", () => {
    const child = el("span", {}, "hello");
    const parent = el("div", {}, child);
    expect(parent.children.length).toBe(1);
    expect(parent.children[0].tagName).toBe("SPAN");
  });

  it("handles mixed string and element children", () => {
    const span = el("span", {}, "world");
    const div = el("div", {}, "hello ", span);
    expect(div.childNodes.length).toBe(2);
    expect(div.childNodes[0].nodeType).toBe(Node.TEXT_NODE);
    expect(div.childNodes[1].nodeType).toBe(Node.ELEMENT_NODE);
  });
});

describe("clear", () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement("div");
  });

  it("removes all children", () => {
    container.appendChild(document.createElement("p"));
    container.appendChild(document.createElement("p"));
    expect(container.childNodes.length).toBe(2);
    clear(container);
    expect(container.childNodes.length).toBe(0);
  });

  it("is safe to call on empty element", () => {
    clear(container);
    expect(container.childNodes.length).toBe(0);
  });
});
