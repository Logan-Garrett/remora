import { describe, it, expect } from "vitest";
import { el } from "../dom";

describe("XSS prevention", () => {
  it("el() does not interpret HTML in string children", () => {
    const div = el("div", {}, "<img src=x onerror=alert(1)>");
    expect(div.children.length).toBe(0); // no img element created
    expect(div.textContent).toBe("<img src=x onerror=alert(1)>");
  });

  it("script tags in text are not executed", () => {
    const div = el("div", {}, "<script>window.__xss=true</script>");
    document.body.appendChild(div);
    expect((window as unknown as Record<string, unknown>).__xss).toBeUndefined();
    document.body.removeChild(div);
  });

  it("event handler attributes in text are not parsed", () => {
    const div = el(
      "div",
      {},
      '<div onmouseover="window.__xss2=true">hover</div>'
    );
    expect(div.children.length).toBe(0);
    expect((window as unknown as Record<string, unknown>).__xss2).toBeUndefined();
  });

  it("nested HTML entities are safe", () => {
    const div = el("div", {}, "&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(div.textContent).toBe(
      "&lt;script&gt;alert(1)&lt;/script&gt;"
    );
  });

  it("textContent assignment is safe for user content", () => {
    const div = document.createElement("div");
    div.textContent = '<a href="javascript:alert(1)">click</a>';
    expect(div.children.length).toBe(0);
    expect(div.innerHTML).toContain("&lt;a");
  });
});
