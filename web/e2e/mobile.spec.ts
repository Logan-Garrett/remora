/**
 * Mobile viewport E2E tests
 *
 * Run against mobile-chrome (Pixel 5) and mobile-safari (iPhone 12) projects.
 * Validates that the core user flows work and the UI is usable at mobile sizes.
 */

import { test, expect, type Page } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";
const DISPLAY_NAME = "playwright-mobile";

async function login(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".login-card");
  await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
  await page.locator('input[type="password"]').fill(TEAM_TOKEN);
  await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
  await page.locator("button.primary").click();
  await page.waitForSelector(".sessions-view", { timeout: 10000 });
}

async function createAndJoinSession(page: Page, desc: string): Promise<void> {
  await page.locator("button.primary", { hasText: "New Session" }).click();
  await page.waitForSelector(".modal");
  await page.locator(".modal input[placeholder*='description']").fill(desc);
  await page.locator(".modal button.primary", { hasText: "Create & Join" }).click();
  await page.waitForSelector(".chat-view", { timeout: 10000 });
}

async function leaveAndDelete(page: Page, desc: string): Promise<void> {
  const chatView = page.locator(".chat-view");
  if (await chatView.isVisible()) {
    await page.locator(".chat-view button", { hasText: "Leave" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
  }
  page.on("dialog", (d) => d.accept());
  const cards = page.locator(".session-card");
  const count = await cards.count();
  for (let i = count - 1; i >= 0; i--) {
    const card = cards.nth(i);
    const text = await card.locator(".session-desc").textContent();
    if (text?.includes(desc)) {
      await card.locator("button.danger").click();
      await page.waitForTimeout(300);
    }
  }
}

test.describe("Mobile — login", () => {
  test("login card fits within viewport", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".login-card");

    const card = page.locator(".login-card");
    const box = await card.boundingBox();
    const viewport = page.viewportSize()!;

    // Card must not overflow horizontally
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
  });

  test("all login fields and button are visible and tappable", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".login-card");

    const serverInput = page.locator('input[placeholder*="server"]');
    const tokenInput = page.locator('input[type="password"]');
    const nameInput = page.locator('input[placeholder="your-name"]');
    const connectBtn = page.locator("button.primary");

    await expect(serverInput).toBeVisible();
    await expect(tokenInput).toBeVisible();
    await expect(nameInput).toBeVisible();
    await expect(connectBtn).toBeVisible();

    // Tap targets: height should be at least 40px on mobile
    const btnBox = await connectBtn.boundingBox();
    expect(btnBox!.height).toBeGreaterThanOrEqual(40);
  });

  test("successful login navigates to sessions view on mobile", async ({ page }) => {
    await login(page);
    await expect(page.locator(".sessions-view")).toBeVisible();
  });

  test("wrong token shows error on mobile", async ({ page }) => {
    await page.goto("/");
    await page.waitForSelector(".login-card");
    await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
    await page.locator('input[type="password"]').fill("wrong-token");
    await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
    await page.locator("button.primary").click();

    const errorLocator = page.locator(".sessions-empty");
    await errorLocator.waitFor({ state: "visible", timeout: 10000 });
    await expect(errorLocator).toContainText("Invalid token");
  });
});

test.describe("Mobile — sessions", () => {
  test("sessions view fits within viewport", async ({ page }) => {
    await login(page);

    const view = page.locator(".sessions-view");
    const box = await view.boundingBox();
    const viewport = page.viewportSize()!;

    expect(box).not.toBeNull();
    expect(box!.width).toBeLessThanOrEqual(viewport.width);
  });

  test("new session modal fits within viewport", async ({ page }) => {
    await login(page);
    await page.locator("button.primary", { hasText: "New Session" }).click();
    await page.waitForSelector(".modal");

    const modal = page.locator(".modal");
    const box = await modal.boundingBox();
    const viewport = page.viewportSize()!;

    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);

    // Close modal
    await page.locator(".modal button", { hasText: "Cancel" }).click();
  });

  test("create session and enter chat on mobile", async ({ page }) => {
    const desc = `mobile-test-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    await expect(page.locator(".chat-view")).toBeVisible();

    await leaveAndDelete(page, desc);
  });
});

test.describe("Mobile — chat", () => {
  test("chat input and send button are visible and usable", async ({ page }) => {
    const desc = `mobile-chat-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    const input = page.locator(".chat-input-bar input");
    const sendBtn = page.locator(".chat-input-bar button.primary");

    await expect(input).toBeVisible();
    await expect(sendBtn).toBeVisible();

    // Input and button must not overflow viewport
    const viewport = page.viewportSize()!;
    const barBox = await page.locator(".chat-input-bar").boundingBox();
    expect(barBox!.x + barBox!.width).toBeLessThanOrEqual(viewport.width);

    await leaveAndDelete(page, desc);
  });

  test("send message on mobile", async ({ page }) => {
    const desc = `mobile-msg-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    const uniqueMsg = `mobile hello ${Date.now()}`;
    const input = page.locator(".chat-input-bar input");
    await input.fill(uniqueMsg);
    await input.press("Enter");

    const msgLocator = page.locator(".chat-messages .chat-event.chat.self .content");
    await expect(msgLocator.first()).toContainText(uniqueMsg, { timeout: 5000 });

    await leaveAndDelete(page, desc);
  });

  test("leave session returns to sessions list on mobile", async ({ page }) => {
    const desc = `mobile-leave-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    await page.locator(".chat-view button", { hasText: "Leave" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
    await expect(page.locator(".sessions-view")).toBeVisible();
    await expect(page.locator(".chat-view")).not.toBeVisible();

    await leaveAndDelete(page, desc);
  });
});
