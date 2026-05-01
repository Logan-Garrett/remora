/** Safe DOM helpers. All user content uses textContent, never innerHTML. */

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Record<string, string>,
  ...children: (string | Node)[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (k === "class") {
        element.className = v;
      } else {
        element.setAttribute(k, v);
      }
    }
  }
  for (const child of children) {
    if (typeof child === "string") {
      element.appendChild(document.createTextNode(child));
    } else {
      element.appendChild(child);
    }
  }
  return element;
}

export function clear(container: HTMLElement): void {
  while (container.firstChild) {
    container.removeChild(container.firstChild);
  }
}

export function $(selector: string): HTMLElement {
  const found = document.querySelector(selector);
  if (!found) throw new Error(`Element not found: ${selector}`);
  return found as HTMLElement;
}
