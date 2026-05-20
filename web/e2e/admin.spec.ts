/**
 * Admin Dashboard E2E tests
 *
 * Covers:
 * - Login with team token (admin), verify Admin button is visible
 * - Click Admin, verify dashboard loads with tabs
 * - Switch between tabs (Overview, Sessions, Users, Audit Log, Allowlist)
 * - Verify Overview shows stat cards
 * - Verify Users tab shows user list
 *
 * Requires the server and Vite dev server to be running.
 * Uses REMORA_SERVER_URL and REMORA_TEAM_TOKEN env vars.
 */

import { test, expect, type Page } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";
const DISPLAY_NAME = "playwright-admin";

async function login(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".login-card");
  await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
  await page.locator('input[type="password"]').fill(TEAM_TOKEN);
  await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
  await page.locator("button.primary").click();
  await page.waitForSelector(".sessions-view", { timeout: 10000 });
}

test.describe("Admin Dashboard", () => {
  test("admin button is visible when logged in with team token", async ({
    page,
  }) => {
    await login(page);
    const adminBtn = page.locator("button.admin-btn");
    await expect(adminBtn).toBeVisible();
    await expect(adminBtn).toHaveText("Admin");
  });

  test("clicking Admin navigates to admin dashboard", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();

    // Admin dashboard should load
    const title = page.locator(".header-title");
    await expect(title).toHaveText("Admin Dashboard", { timeout: 10000 });
  });

  test("admin dashboard shows tab buttons", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    const tabs = page.locator(".admin-tabs .tab");
    await expect(tabs).toHaveCount(5);
    await expect(tabs.nth(0)).toHaveText("Overview");
    await expect(tabs.nth(1)).toHaveText("Sessions");
    await expect(tabs.nth(2)).toHaveText("Users");
    await expect(tabs.nth(3)).toHaveText("Audit Log");
    await expect(tabs.nth(4)).toHaveText("Allowlist");
  });

  test("Overview tab shows stat cards", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    // Overview is the default active tab; stat cards should render
    // after the API calls resolve
    const statCards = page.locator(".stat-card");
    await expect(statCards.first()).toBeVisible({ timeout: 10000 });
    // Should have multiple stat cards (tokens, sessions, active, runs, etc.)
    const count = await statCards.count();
    expect(count).toBeGreaterThanOrEqual(3);
  });

  test("switch to Sessions tab", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    const sessionsTab = page.locator(".admin-tabs .tab", {
      hasText: "Sessions",
    });
    await sessionsTab.click();
    await expect(sessionsTab).toHaveClass(/active/);

    // Sessions tab should show a table or empty state
    const content = page.locator(".admin-content");
    await expect(content).toBeVisible();
  });

  test("switch to Users tab", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    const usersTab = page.locator(".admin-tabs .tab", { hasText: "Users" });
    await usersTab.click();
    await expect(usersTab).toHaveClass(/active/);

    // Users tab should show content
    const content = page.locator(".admin-content");
    await expect(content).toBeVisible();
  });

  test("switch to Audit Log tab", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    const auditTab = page.locator(".admin-tabs .tab", {
      hasText: "Audit Log",
    });
    await auditTab.click();
    await expect(auditTab).toHaveClass(/active/);
  });

  test("switch to Allowlist tab", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    const allowlistTab = page.locator(".admin-tabs .tab", {
      hasText: "Allowlist",
    });
    await allowlistTab.click();
    await expect(allowlistTab).toHaveClass(/active/);
  });

  test("Back to Sessions returns to sessions view", async ({ page }) => {
    await login(page);
    await page.locator("button.admin-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Admin Dashboard", {
      timeout: 10000,
    });

    await page.locator("button", { hasText: "Back to Sessions" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
    await expect(page.locator(".sessions-view")).toBeVisible();
  });
});
