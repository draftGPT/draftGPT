import { defineConfig, devices } from "@playwright/test";

// End-to-end tests against the built apps (`npm run build` first, plus
// `npm run demo -w @draftgpt/desktop` for the desktop demo files).
// Champion icons come from the Data Dragon CDN; tests don't depend on them.
// CHROMIUM_PATH=/usr/bin/chromium uses an installed Chromium instead of
// Playwright's own (`npx playwright install chromium`).
export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  reporter: [["list"]],
  use: {
    ...devices["Desktop Chrome"],
    trace: "retain-on-failure",
    launchOptions: process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {},
  },
  webServer: [
    { command: "npm run preview -w @draftgpt/web", url: "http://localhost:4174", reuseExistingServer: false },
    { command: "npm run preview -w @draftgpt/desktop", url: "http://localhost:4173", reuseExistingServer: false },
  ],
});
