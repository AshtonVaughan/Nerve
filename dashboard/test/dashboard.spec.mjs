// Playwright smoke test for the bundled dashboard.
//
// The test starts the daemon, opens the dashboard at http://127.0.0.1:8765/,
// waits for the live screenshot panel to appear, presses the emergency-stop
// button, and asserts the safety state flips to `emergency_stopped = true`.
//
// Run via `npx playwright test` after `npm install`.

import { test, expect } from "@playwright/test";

test("dashboard connects and renders", async ({ page }) => {
  await page.goto("http://127.0.0.1:8765/");
  await expect(page.locator("h1")).toHaveText("Nerve");
  await expect(page.locator("#platform-pill")).not.toHaveText("…", { timeout: 5_000 });
  // Live screenshot panel should be visible.
  await expect(page.locator(".screenshot h2")).toBeVisible();
});

test("emergency stop sets safety state", async ({ page }) => {
  await page.goto("http://127.0.0.1:8765/");
  await page.locator("#emergency-stop").click();
  await expect(page.locator("#safety-stop")).toHaveText("true", { timeout: 5_000 });
});
