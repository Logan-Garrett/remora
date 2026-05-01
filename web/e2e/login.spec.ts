/**
 * Login flow E2E tests
 *
 * Covers:
 * - Health check gate: /health must return ok before proceeding
 * - Successful login navigates to sessions view
 * - Wrong token shows an error (auth rejection)
 * - Missing fields show a validation error
 */

import { test, expect } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";
const DISPLAY_NAME = "playwright";

test.describe("Login flow", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // The login card must be visible on first load
    await page.waitForSelector(".login-card");
  });

  test("health check: /health endpoint returns ok status", async ({
    request,
  }) => {
    const response = await request.get(`${SERVER_URL}/health`);
    expect(response.ok()).toBe(true);
    const body = await response.json();
    expect(body.status).toBe("ok");
  });

  test("login form renders with expected fields", async ({ page }) => {
    await expect(page.locator("h1")).toHaveText("Remora");
    await expect(page.locator('input[placeholder*="server"]')).toBeVisible();
    await expect(page.locator('input[type="password"]')).toBeVisible();
    await expect(page.locator('input[placeholder="your-name"]')).toBeVisible();
    await expect(page.locator("button.primary")).toBeVisible();
  });

  test("missing fields show validation error", async ({ page }) => {
    await page.locator("button.primary").click();
    await expect(page.locator(".login-error")).toHaveText(
      "All fields are required"
    );
  });

  test("invalid URL format shows error", async ({ page }) => {
    await page.locator('input[placeholder*="server"]').fill("not-a-url");
    await page.locator('input[type="password"]').fill(TEAM_TOKEN);
    await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
    await page.locator("button.primary").click();
    await expect(page.locator(".login-error")).toHaveText(
      "Server URL must start with http:// or https://"
    );
  });

  test("wrong token shows auth error", async ({ page }) => {
    await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
    await page.locator('input[type="password"]').fill("wrong-token");
    await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
    await page.locator("button.primary").click();

    // The sessions view loads but the first API call returns 401
    // The error is shown in the sessions list container
    const errorLocator = page.locator(".sessions-empty");
    await errorLocator.waitFor({ state: "visible", timeout: 10000 });
    await expect(errorLocator).toContainText("Invalid token");
  });

  test("successful login navigates to sessions view", async ({ page }) => {
    await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
    await page.locator('input[type="password"]').fill(TEAM_TOKEN);
    await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
    await page.locator("button.primary").click();

    // Sessions view header must appear
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
    await expect(page.locator(".header-status")).toContainText(DISPLAY_NAME);
  });

  test("unreachable server shows error", async ({ page }) => {
    await page
      .locator('input[placeholder*="server"]')
      .fill("http://127.0.0.1:19999");
    await page.locator('input[type="password"]').fill(TEAM_TOKEN);
    await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
    await page.locator("button.primary").click();

    await expect(page.locator(".login-error")).toContainText(
      "Cannot reach server"
    );
  });
});
