import { expect, test, type Page } from "@playwright/test";

const WEB = "http://localhost:4174";

async function pickChampion(page: Page, name: string) {
  await page.getByLabel("Search champion").fill(name);
  await page.getByRole("option", { name, exact: true }).click();
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (e) => {
    throw e;
  });
  await page.goto(WEB);
});

test("loads the recommender and shows mock suggestions for your role", async ({ page }) => {
  await expect(page.getByRole("region", { name: "Suggested picks" }).locator("[data-champion]").first()).toBeVisible();
  await expect(page.getByText("Mock stats · patch 16.22")).toBeVisible();
  // Slot 1 is top by default: every suggestion's first reason is a top win rate.
  const reasons = page.getByRole("region", { name: "Suggested picks" }).locator(".reasons > span:first-child");
  await expect(reasons.first()).toContainText("win rate Top");
});

test("an enemy pick enters the suggestions as a matchup and can't be picked again", async ({ page, request }) => {
  const picks = page.getByRole("region", { name: "Suggested picks" });
  await expect(picks.locator("[data-champion]").first()).toBeVisible();
  const top = Number(await picks.locator("[data-champion]").first().getAttribute("data-champion"));

  // An enemy the top suggestion has a matchup against, from the served stats.
  type Stats = { champions: { id: number; matchups: { champion: number }[] }[] };
  type ChampionList = { champions: { id: number; name: string }[] };
  const stats = (await (await request.get(`${WEB}/stats/16.22.json`)).json()) as Stats;
  const list = (await (await request.get(`${WEB}/champions.json`)).json()) as ChampionList;
  const enemyId = stats.champions.find((c) => c.id === top)!.matchups[0]!.champion;
  const enemy = list.champions.find((c) => c.id === enemyId)!.name;

  await page.getByLabel("Champion, enemy 1").click();
  await pickChampion(page, enemy);
  await expect(page.getByRole("region", { name: "Enemy team" }).locator('[data-cell="5"]')).toContainText(enemy);
  // The picker moved on to the next enemy slot, where the champion is taken.
  await expect(page.getByRole("option", { name: enemy, exact: true })).toBeDisabled();

  // Either the top suggestion now names the matchup, or it dropped out of view.
  const topRec = picks.locator(`[data-champion="${top}"]`);
  if (await topRec.count()) {
    await expect(topRec).toContainText(enemy);
  } else {
    await expect(picks.locator("[data-champion]").first()).not.toHaveAttribute("data-champion", String(top));
  }
});

test("a ban takes the champion out of the suggestions", async ({ page }) => {
  const picks = page.getByRole("region", { name: "Suggested picks" });
  const top = await picks.locator("[data-champion]").first().getAttribute("data-champion");
  const name = (await picks.locator(".rec .name").first().textContent())!.trim();

  await page.getByLabel("Your bans").locator('[data-ban="0"]').click();
  await pickChampion(page, name);

  await expect(page.getByLabel("Your bans").locator('[data-ban="0"]')).toHaveAttribute("title", name);
  await expect(picks.locator(`[data-champion="${top}"]`)).toHaveCount(0);
});

test("clicking a suggestion picks it for you; changing role changes suggestions", async ({ page }) => {
  const picks = page.getByRole("region", { name: "Suggested picks" });
  const name = (await picks.locator(".rec .name").first().textContent())!.trim();
  await picks.locator(".rec").first().click();
  await expect(page.getByRole("region", { name: "Your team" }).locator('[data-cell="0"]')).toContainText(name);

  await page.getByLabel("Role, ally 1").selectOption("utility");
  await expect(picks.locator(".reasons > span:first-child").first()).toContainText("win rate Support");
});

test("roles swap within a team and You moves to another slot", async ({ page }) => {
  const picks = page.getByRole("region", { name: "Suggested picks" });
  await expect(page.getByLabel("You are ally 1")).toHaveAttribute("aria-pressed", "true");

  // Taking ally 3's role (mid) gives ally 3 the old one (top).
  await page.getByLabel("Role, ally 1").selectOption("middle");
  await expect(page.getByLabel("Role, ally 3")).toHaveValue("top");

  // Moving You to ally 3: suggestions follow that slot's role.
  await page.getByLabel("You are ally 3").click();
  await expect(page.getByLabel("You are ally 3")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByLabel("You are ally 1")).toHaveAttribute("aria-pressed", "false");
  await expect(picks.locator(".reasons > span:first-child").first()).toContainText("win rate Top");

  // Enemy roles start unknown and stay separate from your team's.
  await expect(page.getByLabel("Role, enemy 1")).toHaveValue("");
  await page.getByLabel("Role, enemy 1").selectOption("middle");
  await expect(page.getByLabel("Role, ally 1")).toHaveValue("middle");
});

test("ranked: your bans, then the enemy's revealed, then picks in order; a ban can be skipped", async ({ page }) => {
  const banner = page.locator(".banner");
  await expect(banner).toContainText("Ban phase · Your ban");
  for (const name of ["Ahri", "Zed", "Yasuo", "Yone"]) await pickChampion(page, name);
  await page.getByRole("button", { name: "No ban" }).click();
  await expect(banner).toContainText("Bans revealed · Enemy ban");
  // Blind bans: the enemy may ban one of yours again.
  for (const name of ["Ahri", "Jinx", "Thresh", "Vi", "Garen"]) await pickChampion(page, name);
  await expect(page.getByLabel("Your bans").locator('[data-ban="0"]')).toHaveAttribute("title", "Ahri");
  await expect(page.getByLabel("Enemy bans").locator('[data-ban="0"]')).toHaveAttribute("title", "Ahri");
  // Blue picks first: you (blue 1).
  await expect(banner).toContainText("Pick phase · Your pick");
  await pickChampion(page, "Darius");
  await expect(banner).toContainText("Pick phase · Enemy pick");
  await expect(page.getByLabel("Champion, enemy 1")).toHaveCount(1);
  await expect(page.locator('.enemy [data-cell="5"]')).toHaveClass(/picking/);
  await expect(page.locator('.enemy [data-cell="6"]')).toHaveClass(/picking/);
  await expect(page.getByLabel("Your bans").locator('[data-ban="4"]')).toHaveAttribute("title", "Set ban");
});

test("tournament on red: bans alternate from blue, two phases each", async ({ page }) => {
  await page.getByRole("button", { name: "Tournament" }).click();
  await page.getByRole("button", { name: "Red" }).click();
  const banner = page.locator(".banner");
  await expect(page.getByRole("region", { name: "Your team" })).toContainText("Red side");
  await expect(banner).toContainText("Ban phase 1 · Enemy ban");
  await pickChampion(page, "Ahri");
  await expect(banner).toContainText("Ban phase 1 · Your ban");
  for (const name of ["Zed", "Yasuo", "Yone", "Lux", "Jinx"]) await pickChampion(page, name);
  await expect(banner).toContainText("Pick phase 1 · Enemy pick");
  await pickChampion(page, "Garen");
  await expect(banner).toContainText("Pick phase 1 · Your pick");
  for (const name of ["Darius", "Vi", "Thresh", "Leona", "Jax"]) await pickChampion(page, name);
  // Phase 2 bans start with red; the captain (slot 1, you) makes every ban.
  await expect(banner).toContainText("Ban phase 2 · Your ban");
});

test("fearless: the next game's picker and suggestions leave out earlier picks", async ({ page }) => {
  await page.getByRole("button", { name: "Fearless" }).click();
  const banner = page.locator(".banner");
  await expect(banner).toContainText("Game 1 · Ban phase 1 · Your ban");
  for (const name of ["Ahri", "Zed", "Yasuo", "Yone", "Lux", "Jinx"]) await pickChampion(page, name);
  await pickChampion(page, "Garen"); // blue pick 1: you
  await page.getByRole("button", { name: "Next game" }).click();
  await expect(banner).toContainText("Game 2 · Ban phase 1");
  const row = page.getByRole("group", { name: "Fearless" });
  await expect(row.locator("[data-champion]")).toHaveCount(1);
  await expect(row.locator('button[title^="Garen"]')).toHaveCount(1);
  await expect(page.getByRole("option", { name: "Garen", exact: true })).toBeDisabled();
  // Ahri was only banned in game 1: free again.
  await expect(page.getByRole("option", { name: "Ahri", exact: true })).toBeEnabled();

  // Add one by hand, then take it off again.
  await page.getByLabel("Add a champion from an earlier game").click();
  await pickChampion(page, "Darius");
  await expect(row.locator("[data-champion]")).toHaveCount(2);
  await page.getByLabel("Remove Darius").click();
  await expect(row.locator("[data-champion]")).toHaveCount(1);
  const picks = page.getByRole("region", { name: "Suggested picks" });
  await expect(picks.getByText("Garen", { exact: true })).toHaveCount(0);
});

test("reset clears the draft", async ({ page }) => {
  await page.getByLabel("Champion, ally 2").click();
  await pickChampion(page, "Ahri");
  await page.getByRole("button", { name: "Reset" }).click();
  await expect(page.getByRole("region", { name: "Your team" })).not.toContainText("Ahri");
});

test("filling bans out of order never overwrites one already set", async ({ page }) => {
  const bans = page.getByLabel("Your bans");
  await bans.locator('[data-ban="2"]').click();
  for (const name of ["Ahri", "Zed", "Lux"]) await pickChampion(page, name);
  // Ahri went to spot 3, then the picker filled the first empty spots.
  await expect(bans.locator('[data-ban="2"]')).toHaveAttribute("title", "Ahri");
  await expect(bans.locator('[data-ban="0"]')).toHaveAttribute("title", "Zed");
  await expect(bans.locator('[data-ban="1"]')).toHaveAttribute("title", "Lux");
});

test("a broken cached stats file is dropped and fetched again", async ({ page, request }) => {
  const index = (await (await request.get(`${WEB}/stats/index.json`)).json()) as { latest: string; hash: string };
  const key = `draftgpt.stats.${index.latest}.${index.hash}`;
  // Let the first load finish caching, so it can't overwrite the broken entry.
  await expect(page.getByRole("region", { name: "Suggested picks" }).locator("[data-champion]").first()).toBeVisible();
  await page.evaluate((k) => localStorage.setItem(k, "<!doctype html><title>not stats</title>"), key);
  expect(await page.evaluate((k) => localStorage.getItem(k), key)).toContain("not stats");
  await page.reload();
  await expect(page.getByRole("region", { name: "Suggested picks" }).locator("[data-champion]").first()).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  const cached = await page.evaluate((k) => localStorage.getItem(k), `draftgpt.stats.${index.latest}.${index.hash}`);
  expect(cached?.startsWith("{")).toBe(true);
});
