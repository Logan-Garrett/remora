import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  // Each test file runs in its own worker to avoid state leakage
  fullyParallel: false,
  // Fail the build on CI if test.only is accidentally left in
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    // Base URL for the Vite dev server
    baseURL: process.env.VITE_BASE_URL ?? "http://localhost:3333",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      testIgnore: /mobile\.spec\.ts/,
    },
    // iOS — oldest to newest available in Playwright
    {
      name: "iPhone 12",
      use: { ...devices["iPhone 12"] },
      testMatch: /mobile\.spec\.ts/,
    },
    {
      name: "iPhone 15 Pro",
      use: { ...devices["iPhone 15 Pro"] },
      testMatch: /mobile\.spec\.ts/,
    },
    {
      name: "iPhone 15 Pro Max",
      use: { ...devices["iPhone 15 Pro Max"] },
      testMatch: /mobile\.spec\.ts/,
    },
    // Android — Pixel and Samsung flagship
    {
      name: "Pixel 5",
      use: { ...devices["Pixel 5"] },
      testMatch: /mobile\.spec\.ts/,
    },
    {
      name: "Pixel 7",
      use: { ...devices["Pixel 7"] },
      testMatch: /mobile\.spec\.ts/,
    },
    {
      name: "Galaxy S24",
      use: { ...devices["Galaxy S24"] },
      testMatch: /mobile\.spec\.ts/,
    },
  ],
  // No webServer block — CI starts servers separately so they are already up
});
