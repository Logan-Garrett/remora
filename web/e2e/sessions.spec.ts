/**
 * Session CRUD E2E tests
 *
 * Covers:
 * - Create a session with a description → it appears in the list
 * - Delete a session → it disappears from the list
 * - Leave a session (chat view) → returns to sessions list
 * - Rejoin the same session
 *
 * Each test logs in fresh and cleans up any sessions it creates.
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

async function openCreateModal(page: Page): Promise<void> {
  await page.locator("button.primary", { hasText: "New Session" }).click();
  await page.waitForSelector(".modal");
}

async function createSession(page: Page, description: string): Promise<void> {
  await openCreateModal(page);
  await page.locator(".modal input[placeholder*='description']").fill(description);
  await page.locator(".modal button.primary", { hasText: "Create & Join" }).click();
  // After creation we land in the chat view
  await page.waitForSelector(".chat-view", { timeout: 10000 });
}

async function leaveSession(page: Page): Promise<void> {
  await page.locator(".chat-view button", { hasText: "Leave" }).click();
  await page.waitForSelector(".sessions-view", { timeout: 10000 });
}

/** Delete all sessions created during a test by their description prefix. */
async function deleteSessionByDesc(
  page: Page,
  desc: string
): Promise<void> {
  // Dismiss any open dialogs first
  page.on("dialog", (dialog) => dialog.accept());
  const cards = page.locator(".session-card");
  const count = await cards.count();
  for (let i = count - 1; i >= 0; i--) {
    const card = cards.nth(i);
    const text = await card.locator(".session-desc").textContent();
    if (text?.includes(desc)) {
      await card.locator("button.danger").click();
      // Wait for the card to disappear
      await page.waitForTimeout(300);
    }
  }
}

test.describe("Sessions CRUD", () => {
  const DESC_PREFIX = "e2e-sessions-test";

  test("create session → appears in list after leaving", async ({ page }) => {
    const desc = `${DESC_PREFIX}-create-${Date.now()}`;
    await login(page);
    await createSession(page, desc);
    // Leave back to sessions list
    await leaveSession(page);
    // The session card should be visible
    await expect(page.locator(".session-desc", { hasText: desc })).toBeVisible();
    // Cleanup
    await deleteSessionByDesc(page, desc);
  });

  test("create session modal requires description", async ({ page }) => {
    await login(page);
    await openCreateModal(page);
    // Try to submit without a description
    await page.locator(".modal button.primary", { hasText: "Create & Join" }).click();
    await expect(page.locator(".modal .login-error")).toHaveText(
      "Description is required"
    );
    // Close modal
    await page.locator(".modal button", { hasText: "Cancel" }).click();
  });

  test("cancel modal closes without creating a session", async ({ page }) => {
    await login(page);
    // Note the current count
    const before = await page.locator(".session-card").count();
    await openCreateModal(page);
    await page.locator(".modal button", { hasText: "Cancel" }).click();
    await expect(page.locator(".modal")).not.toBeVisible();
    const after = await page.locator(".session-card").count();
    expect(after).toBe(before);
  });

  test("delete session → removed from list", async ({ page }) => {
    const desc = `${DESC_PREFIX}-delete-${Date.now()}`;
    await login(page);
    await createSession(page, desc);
    await leaveSession(page);

    // Confirm the card exists
    await expect(page.locator(".session-desc", { hasText: desc })).toBeVisible();

    // Set up dialog handler before clicking delete
    page.on("dialog", (dialog) => dialog.accept());
    const card = page.locator(".session-card", { hasText: desc });
    await card.locator("button.danger").click();

    // Card should disappear
    await expect(
      page.locator(".session-desc", { hasText: desc })
    ).not.toBeVisible({ timeout: 5000 });
  });

  test("leave session returns to sessions list", async ({ page }) => {
    const desc = `${DESC_PREFIX}-leave-${Date.now()}`;
    await login(page);
    await createSession(page, desc);

    // We're now in the chat view
    await expect(page.locator(".chat-view")).toBeVisible();

    await leaveSession(page);

    // Back in sessions view
    await expect(page.locator(".sessions-view")).toBeVisible();
    await expect(page.locator(".chat-view")).not.toBeVisible();

    // Cleanup
    await deleteSessionByDesc(page, desc);
  });

  test("rejoin session works after leaving", async ({ page }) => {
    const desc = `${DESC_PREFIX}-rejoin-${Date.now()}`;
    await login(page);
    await createSession(page, desc);
    await leaveSession(page);

    // Click on the session card to rejoin
    const card = page.locator(".session-card", { hasText: desc });
    await card.click();

    // Chat view should appear again
    await page.waitForSelector(".chat-view", { timeout: 10000 });
    await expect(page.locator(".header-title")).toContainText(desc);

    // Cleanup
    await leaveSession(page);
    await deleteSessionByDesc(page, desc);
  });

  test("disconnect button returns to login screen", async ({ page }) => {
    await login(page);
    await page.locator("button", { hasText: "Disconnect" }).click();
    await page.waitForSelector(".login-card", { timeout: 5000 });
    await expect(page.locator(".login-card")).toBeVisible();
  });
});
