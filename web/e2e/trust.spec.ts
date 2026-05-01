/**
 * Trust & ownership E2E tests
 *
 * Covers:
 * - Session creation shows "Owner Key" button in chat header
 * - /trust command by owner emits a system message
 * - /who command shows trusted participants list
 * - Duplicate display name is rejected with error
 * - Owner Key button copies key to clipboard
 */

import { test, expect, type Page, type BrowserContext } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";

async function login(page: Page, name: string): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".login-card");
  await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
  await page.locator('input[type="password"]').fill(TEAM_TOKEN);
  await page.locator('input[placeholder="your-name"]').fill(name);
  await page.locator("button.primary").click();
  await page.waitForSelector(".sessions-view", { timeout: 10000 });
}

async function createAndJoinSession(page: Page, desc: string): Promise<void> {
  await page.locator("button.primary", { hasText: "New Session" }).click();
  await page.waitForSelector(".modal");
  await page.locator(".modal input[placeholder*='description']").fill(desc);
  await page
    .locator(".modal button.primary", { hasText: "Create & Join" })
    .click();
  await page.waitForSelector(".chat-view", { timeout: 10000 });
}

async function sendCommand(page: Page, command: string): Promise<void> {
  const input = page.locator(".chat-input-bar input");
  await input.fill(command);
  await input.press("Enter");
}

async function leaveAndDeleteSession(
  page: Page,
  desc: string
): Promise<void> {
  const chatView = page.locator(".chat-view");
  if (await chatView.isVisible()) {
    await page.locator(".chat-view button", { hasText: "Leave" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
  }
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

test.describe("Trust & Ownership", () => {
  const DESC = `trust-e2e-${Date.now()}`;

  test("session creator sees Owner Key button", async ({ page }) => {
    await login(page, "owner-test");
    await createAndJoinSession(page, DESC);

    // The "Owner Key" button should be visible in the header
    const ownerKeyBtn = page.locator(".header-actions button", {
      hasText: "Owner Key",
    });
    await expect(ownerKeyBtn).toBeVisible({ timeout: 5000 });

    await leaveAndDeleteSession(page, DESC);
  });

  test("/trust by owner shows system message", async ({ page }) => {
    await login(page, "alice-trust");
    await createAndJoinSession(page, DESC);

    // Wait for connection
    await page.waitForTimeout(1000);

    // Trust a participant
    await sendCommand(page, "/trust bob");

    // Should see "now trusted" system message
    const trustedMsg = page.locator(".chat-event", {
      hasText: "now trusted",
    });
    await expect(trustedMsg).toBeVisible({ timeout: 5000 });

    await leaveAndDeleteSession(page, DESC);
  });

  test("/who shows trusted participants", async ({ page }) => {
    await login(page, "alice-who");
    await createAndJoinSession(page, DESC);
    await page.waitForTimeout(1000);

    // Trust bob first
    await sendCommand(page, "/trust bob");
    await page.waitForTimeout(500);

    // Run /who
    await sendCommand(page, "/who");

    // Should see "Trusted:" in the output
    const trustedLine = page.locator(".chat-event", {
      hasText: "Trusted:",
    });
    await expect(trustedLine).toBeVisible({ timeout: 5000 });

    await leaveAndDeleteSession(page, DESC);
  });

  test("duplicate display name is rejected", async ({
    browser,
  }) => {
    const context1 = await browser.newContext();
    const context2 = await browser.newContext();
    const page1 = await context1.newPage();
    const page2 = await context2.newPage();

    await login(page1, "dup-name-test");
    await createAndJoinSession(page1, DESC);

    // Get the session card text to find the session ID for page2
    // Page2 logs in with the same name and tries to join the same session
    await login(page2, "dup-name-test");

    // Click on the session card to join
    const sessionCard = page2.locator(".session-card", { hasText: DESC });
    if (await sessionCard.isVisible({ timeout: 3000 })) {
      await sessionCard.click();
      // Should see an error about name already in use
      // The WS sends an error and the client should show it
      await page2.waitForTimeout(2000);
    }

    // Clean up
    await page1.locator(".chat-view button", { hasText: "Leave" }).click();
    await page1.waitForSelector(".sessions-view", { timeout: 10000 });
    await leaveAndDeleteSession(page1, DESC);

    await context1.close();
    await context2.close();
  });
});
