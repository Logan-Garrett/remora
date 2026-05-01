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
    {
      name: "mobile-chrome",
      use: { ...devices["Pixel 5"] },
      testMatch: /mobile\.spec\.ts/,
    },
    {
      name: "mobile-safari",
      use: { ...devices["iPhone 12"] },
      testMatch: /mobile\.spec\.ts/,
    },
  ],
  // No webServer block — CI starts servers separately so they are already up
});
