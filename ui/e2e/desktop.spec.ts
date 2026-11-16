import { expect, test } from "@playwright/test";

const DESKTOP = "http://localhost:4173";

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (e) => {
    throw e;
  });
});

test("outside the app and without a demo it says how to run", async ({ page }) => {
  await page.goto(DESKTOP);
  await expect(page.getByText("Not running in the desktop app")).toBeVisible();
});

test("replays a ranked-style draft: turns, your pick, the lock-in and recommendations", async ({ page }) => {
  // 48 s of recording at 2x: your ban is on screen from about 4.4 s to 5.9 s.
  test.setTimeout(60_000);
  await page.goto(`${DESKTOP}/?demo=16_22_draft_ban_and_pick_timeout_autolock&speed=2`);
  await expect(page.getByText("Demo: 16_22_draft_ban_and_pick_timeout_autolock")).toBeVisible();
  await expect(page.getByRole("region", { name: "Your team" }).locator(".slot")).toHaveCount(5);
  await expect(page.getByRole("region", { name: "Enemy team" }).locator(".slot")).toHaveCount(5);
  // Somewhere in the draft it's your ban, then your pick.
  await expect(page.getByText("Your ban")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("region", { name: "Suggested bans" }).locator("[data-champion]").first()).toBeVisible();
  await expect(page.getByText("Your pick")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("region", { name: "Suggested picks" }).locator("[data-champion]").first()).toBeVisible();
  // It ends in finalization with your champion locked, then champ select ends.
  await expect(page.getByText("Finalization")).toBeVisible({ timeout: 20_000 });
  await expect(page.locator(".slot.local .name")).not.toHaveClass(/muted/);
  await expect(page.getByText("Waiting for champ select…")).toBeVisible({ timeout: 10_000 });
});

test("a spectated tournament draft shows both sides and ten bans", async ({ page }) => {
  await page.goto(`${DESKTOP}/?demo=rcvolus_2019_tournament_draft_spectated&speed=200`);
  await expect(page.getByRole("region", { name: "Blue side" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Red side" })).toBeVisible();
  await expect(page.getByLabel("Your bans").locator(".spot")).toHaveCount(5, { timeout: 10_000 });
  await expect(page.getByLabel("Enemy bans").locator(".spot")).toHaveCount(5);
  await expect(page.locator(".slot .name.muted")).toHaveCount(0, { timeout: 10_000 });
});

test("ARAM shows the bench and rerolls", async ({ page }) => {
  // 29 s of recording at 5x.
  await page.goto(`${DESKTOP}/?demo=16_22_aram_bench_swaps&speed=5`);
  await expect(page.getByText("Bench", { exact: true })).toBeVisible();
  await expect(page.getByText("Rerolls", { exact: true })).toBeVisible();
});
