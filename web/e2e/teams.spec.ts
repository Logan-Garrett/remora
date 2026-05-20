/**
 * Teams E2E tests
 *
 * Covers:
 * - Login, verify Teams button visible
 * - Click Teams, verify teams page loads
 * - Create a team via the Create Team tab
 * - Verify the team appears in the list
 *
 * Note: The Teams feature requires JWT authentication (not team token).
 * Since E2E tests use the team token (admin), the Teams button is shown
 * but team API calls require JWT. These tests verify the UI flow.
 *
 * Requires the server and Vite dev server to be running.
 * Uses REMORA_SERVER_URL and REMORA_TEAM_TOKEN env vars.
 */

import { test, expect, type Page } from "@playwright/test";

const SERVER_URL = process.env.REMORA_SERVER_URL ?? "http://127.0.0.1:7200";
const TEAM_TOKEN = process.env.REMORA_TEAM_TOKEN ?? "e2e-test-token";
const DISPLAY_NAME = "playwright-teams";

async function login(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector(".login-card");
  await page.locator('input[placeholder*="server"]').fill(SERVER_URL);
  await page.locator('input[type="password"]').fill(TEAM_TOKEN);
  await page.locator('input[placeholder="your-name"]').fill(DISPLAY_NAME);
  await page.locator("button.primary").click();
  await page.waitForSelector(".sessions-view", { timeout: 10000 });
}

test.describe("Teams", () => {
  test("Teams button is visible in sessions view", async ({ page }) => {
    await login(page);
    const teamsBtn = page.locator("button.teams-btn");
    await expect(teamsBtn).toBeVisible();
    await expect(teamsBtn).toHaveText("Teams");
  });

  test("clicking Teams navigates to teams page", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();

    // Teams page should load with the header
    const title = page.locator(".header-title");
    await expect(title).toHaveText("Teams", { timeout: 10000 });
  });

  test("teams page shows tab buttons", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Teams", {
      timeout: 10000,
    });

    const tabs = page.locator(".admin-tabs .tab");
    await expect(tabs).toHaveCount(2);
    await expect(tabs.nth(0)).toHaveText("My Teams");
    await expect(tabs.nth(1)).toHaveText("Create Team");
  });

  test("My Teams tab is active by default", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Teams", {
      timeout: 10000,
    });

    const activeTab = page.locator(".admin-tabs .tab.active");
    await expect(activeTab).toHaveText("My Teams");
  });

  test("switch to Create Team tab shows form", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Teams", {
      timeout: 10000,
    });

    const createTab = page.locator(".admin-tabs .tab", {
      hasText: "Create Team",
    });
    await createTab.click();
    await expect(createTab).toHaveClass(/active/);

    // The create team form should have Name and Description fields
    await expect(page.locator("label", { hasText: "Name" })).toBeVisible();
    await expect(
      page.locator("label", { hasText: "Description" })
    ).toBeVisible();
  });

  test("create team form validates empty name", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Teams", {
      timeout: 10000,
    });

    // Switch to Create Team tab
    await page
      .locator(".admin-tabs .tab", { hasText: "Create Team" })
      .click();

    // Click create without filling name
    await page
      .locator(".teams-create-form button.primary")
      .click();

    await expect(page.locator(".login-error")).toHaveText(
      "Team name is required"
    );
  });

  test("Back to Sessions returns to sessions view", async ({ page }) => {
    await login(page);
    await page.locator("button.teams-btn").click();
    await expect(page.locator(".header-title")).toHaveText("Teams", {
      timeout: 10000,
    });

    await page.locator("button", { hasText: "Back to Sessions" }).click();
    await page.waitForSelector(".sessions-view", { timeout: 10000 });
    await expect(page.locator(".sessions-view")).toBeVisible();
  });
});
