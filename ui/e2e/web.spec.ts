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

  await page.getByRole("region", { name: "Enemy team" }).locator('[data-cell="5"]').click();
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

  await page.getByLabel("Your role").selectOption("utility");
  await expect(picks.locator(".reasons > span:first-child").first()).toContainText("win rate Support");
});

test("reset clears the draft", async ({ page }) => {
  await page.getByRole("region", { name: "Your team" }).locator('[data-cell="1"]').click();
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
  await page.evaluate(
    ([key]) => localStorage.setItem(key!, "<!doctype html><title>not stats</title>"),
    [`draftgpt.stats.${index.latest}.${index.hash}`],
  );
  await page.reload();
  await expect(page.getByRole("region", { name: "Suggested picks" }).locator("[data-champion]").first()).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  const cached = await page.evaluate((k) => localStorage.getItem(k), `draftgpt.stats.${index.latest}.${index.hash}`);
  expect(cached?.startsWith("{")).toBe(true);
});
