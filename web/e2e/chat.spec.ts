/**
 * Chat E2E tests
 *
 * Covers:
 * - Send a plain chat message → appears in message list
 * - /help command → server responds with help text
 * - WebSocket connectivity (status shows "Connected")
 */

import { test, expect, type Page } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";
const DISPLAY_NAME = "playwright";

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

async function leaveAndDeleteSession(page: Page, desc: string): Promise<void> {
  // Leave if in chat view
  const chatView = page.locator(".chat-view");
  if (await chatView.isVisible()) {
    await page.locator(".chat-view button", { hasText: "Leave" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
  }
  // Delete the session
  page.on("dialog", (dialog) => dialog.accept());
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

async function sendMessage(page: Page, text: string): Promise<void> {
  const input = page.locator(".chat-input-bar input");
  await input.fill(text);
  await input.press("Enter");
}

test.describe("Chat", () => {
  const DESC_PREFIX = "e2e-chat-test";

  test("WebSocket connects and shows Connected status", async ({ page }) => {
    const desc = `${DESC_PREFIX}-ws-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    // Status text should say Connected (set synchronously in renderChat)
    await expect(page.locator(".header-status")).toHaveText("Connected");

    await leaveAndDeleteSession(page, desc);
  });

  test("send a plain chat message → appears in messages list", async ({
    page,
  }) => {
    const desc = `${DESC_PREFIX}-msg-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    const uniqueMsg = `hello e2e ${Date.now()}`;
    await sendMessage(page, uniqueMsg);

    // The message should appear in the chat messages area
    const msgLocator = page.locator(".chat-messages .chat-event.chat.self .content");
    await expect(msgLocator.first()).toContainText(uniqueMsg, { timeout: 5000 });

    await leaveAndDeleteSession(page, desc);
  });

  test("send message via Send button (not Enter)", async ({ page }) => {
    const desc = `${DESC_PREFIX}-btn-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    const uniqueMsg = `send-button-test ${Date.now()}`;
    await page.locator(".chat-input-bar input").fill(uniqueMsg);
    await page.locator(".chat-input-bar button.primary", { hasText: "Send" }).click();

    const msgLocator = page.locator(".chat-messages .chat-event.chat.self .content");
    await expect(msgLocator.first()).toContainText(uniqueMsg, { timeout: 5000 });

    await leaveAndDeleteSession(page, desc);
  });

  test("/help command → server responds with system help event", async ({
    page,
  }) => {
    const desc = `${DESC_PREFIX}-help-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    await sendMessage(page, "/help");

    // The server should send back a system event containing help text.
    // We look for any .chat-event that contains common help keywords.
    const systemEvents = page.locator(".chat-messages .chat-event");
    await expect(
      systemEvents.filter({ hasText: /\/help|\/run|Available commands/i }).first()
    ).toBeVisible({ timeout: 8000 });

    await leaveAndDeleteSession(page, desc);
  });

  test("/who command → server responds with presence info", async ({
    page,
  }) => {
    const desc = `${DESC_PREFIX}-who-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    await sendMessage(page, "/who");

    // Server should return a system event with the user's name
    const systemEvents = page.locator(".chat-messages .chat-event");
    await expect(
      systemEvents.filter({ hasText: new RegExp(DISPLAY_NAME, "i") }).first()
    ).toBeVisible({ timeout: 8000 });

    await leaveAndDeleteSession(page, desc);
  });

  test("joining shows a 'joined' system event", async ({ page }) => {
    const desc = `${DESC_PREFIX}-join-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    // The server sends a "joined" system event when a user connects
    const systemEvents = page.locator(".chat-messages .chat-event");
    await expect(
      systemEvents.filter({ hasText: /joined/i }).first()
    ).toBeVisible({ timeout: 8000 });

    await leaveAndDeleteSession(page, desc);
  });

  test("input is cleared after sending", async ({ page }) => {
    const desc = `${DESC_PREFIX}-clear-${Date.now()}`;
    await login(page);
    await createAndJoinSession(page, desc);

    const input = page.locator(".chat-input-bar input");
    await input.fill("some text to clear");
    await input.press("Enter");

    // Input should be empty now
    await expect(input).toHaveValue("");

    await leaveAndDeleteSession(page, desc);
  });
});
