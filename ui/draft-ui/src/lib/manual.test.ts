import { describe, expect, it } from "vitest";
import {
  addFearless,
  draftOrder,
  fromReplay,
  nextGame,
  resumeStep,
  removeFearless,
  newManualDraft,
  nextOpen,
  setBan,
  setChampion,
  setLocal,
  setPosition,
  taken,
  withTurn,
  type Step,
} from "./manual";

describe("manual draft", () => {
  it("starts with five slots a side, roles for your team and you in slot 1", () => {
    const d = newManualDraft();
    expect(d.ally.map((s) => s.position)).toEqual(["top", "jungle", "middle", "bottom", "utility"]);
    expect(d.enemy.map((s) => s.cellId)).toEqual([5, 6, 7, 8, 9]);
    expect(d.ally.filter((s) => s.isLocal).map((s) => s.cellId)).toEqual([0]);
  });

  it("never changes its input", () => {
    const d = newManualDraft();
    const copy = structuredClone(d);
    setChampion(d, "ally", 0, 103);
    setBan(d, "enemy", 2, 157);
    setLocal(d, 3);
    expect(d).toEqual(copy);
  });

  it("moves a champion instead of duplicating it", () => {
    let d = setChampion(newManualDraft(), "ally", 0, 103);
    d = setChampion(d, "enemy", 7, 103);
    expect(d.ally[0]!.champion).toBeNull();
    expect(d.enemy[2]!.champion).toBe(103);
    d = setBan(d, "ally", 0, 103);
    expect(d.enemy[2]!.champion).toBeNull();
    expect([...taken(d)]).toEqual([103]);
  });

  it("fills gaps before a later ban with empty bans and drops empty bans at the end", () => {
    let d = setBan(newManualDraft(), "ally", 2, 1);
    expect(d.bans.ally.map((b) => b.champion)).toEqual([null, null, 1]);
    d = setBan(d, "ally", 2, null);
    expect(d.bans.ally[0]?.champion ?? null).toBeNull();
  });

  it("sets who you are and your role", () => {
    let d = setLocal(newManualDraft(), 4);
    d = setPosition(d, "ally", 4, null);
    expect(d.ally.find((s) => s.isLocal)).toMatchObject({ cellId: 4, position: null });
  });

  it("swaps positions so a team has each one once", () => {
    let d = setPosition(newManualDraft(), "ally", 0, "utility");
    expect(d.ally.map((s) => s.position)).toEqual(["utility", "jungle", "middle", "bottom", "top"]);
    d = setPosition(d, "ally", 1, null);
    d = setPosition(d, "ally", 2, "jungle");
    expect(d.ally.map((s) => s.position)).toEqual(["utility", null, "jungle", "bottom", "top"]);
  });

  it("gives enemies positions without touching your team", () => {
    let d = setPosition(newManualDraft(), "enemy", 5, "middle");
    d = setPosition(d, "enemy", 6, "middle");
    expect(d.enemy.map((s) => s.position)).toEqual([null, "middle", null, null, null]);
    expect(d.ally.map((s) => s.position)).toEqual(newManualDraft().ally.map((s) => s.position));
  });

});

/** "Bb0" = blue ban index 0, "Rp7" = red pick cell 7, as map sides (you on blue: ally = blue). */
const short = (steps: Step[], you: "blue" | "red") =>
  steps.map((s) => {
    const team = (s.side === "ally") === (you === "blue") ? "B" : "R";
    return s.kind === "ban" ? `${team}b${s.index}` : `${team}p${s.cellId % 5}`;
  });

describe("draft order", () => {
  it("ranked: your five bans together, the enemy's revealed together, then picks 1-2-2-2-2-1", () => {
    const steps = draftOrder("ranked", "red");
    expect(steps.slice(0, 5).every((s) => s.side === "ally" && s.group === 0)).toBe(true);
    expect(steps.slice(5, 10).every((s) => s.side === "enemy" && s.group === 1)).toBe(true);
    expect(short(steps.slice(10), "red").join(" ")).toBe("Bp0 Rp0 Rp1 Bp1 Bp2 Rp2 Rp3 Bp3 Bp4 Rp4");
    const groups = steps.slice(10).map((s) => s.group);
    expect(new Set(groups).size).toBe(6);
  });

  it("tournament: bans B R B R B R, picks B R R B B R, bans R B R B, picks R B B R", () => {
    const steps = draftOrder("tournament", "blue");
    expect(short(steps, "blue").join(" ")).toBe(
      "Bb0 Rb0 Bb1 Rb1 Bb2 Rb2 Bp0 Rp0 Rp1 Bp1 Bp2 Rp2 Rb3 Bb3 Rb4 Bb4 Rp3 Bp3 Bp4 Rp4",
    );
    expect([...new Set(steps.map((s) => s.phase))]).toEqual(["Ban phase 1", "Pick phase 1", "Ban phase 2", "Pick phase 2"]);
  });

  it("ranked bans are blind: both teams can ban the same champion, one team can't twice", () => {
    let d = setBan(newManualDraft(), "ally", 0, 157, { blind: true });
    d = setBan(d, "enemy", 0, 157, { blind: true });
    expect([d.bans.ally[0]!.champion, d.bans.enemy[0]!.champion]).toEqual([157, 157]);
    expect(taken(d, "enemy").has(157)).toBe(true);
    expect(taken(newManualDraft(), "enemy").size).toBe(0);
    const onlyAlly = setBan(newManualDraft(), "ally", 0, 157, { blind: true });
    expect(taken(onlyAlly, "enemy").has(157)).toBe(false);
    d = setBan(d, "ally", 1, 157, { blind: true });
    expect(d.bans.ally.map((b) => b.champion)).toEqual([null, 157]);
    // Not blind (tournament): banning moves the champion.
    const t = setBan(setBan(newManualDraft(), "ally", 0, 157), "enemy", 0, 157);
    expect([t.bans.ally[0]?.champion ?? null, t.bans.enemy[0]!.champion]).toEqual([null, 157]);
  });

  it("tournament bans belong to each team's captain (slot 1)", () => {
    const steps = draftOrder("tournament", "blue");
    const d = newManualDraft(3);
    const ban = steps.findIndex((s) => s.kind === "ban" && s.side === "ally");
    expect(withTurn(d, steps, ban).turn).toEqual({ kind: "ban", cells: [0], yours: false });
    expect(withTurn(newManualDraft(0), steps, ban).turn).toMatchObject({ yours: true });
    expect(steps.filter((s) => s.kind === "ban").map((s) => s.cellId % 5)).toEqual(Array(10).fill(0));
  });

  it("turns follow the steps: yours when you're in the group, finalization at the end", () => {
    const steps = draftOrder("ranked", "blue");
    let d = newManualDraft(2);
    expect(withTurn(d, steps, 0).turn).toEqual({ kind: "ban", cells: [0, 1, 2, 3, 4], yours: true });
    expect(withTurn(d, steps, 5).turn).toMatchObject({ kind: "ban", yours: false });
    // Blue picks 2 and 3 (cells 1, 2) are one turn; you're cell 2.
    const i = steps.findIndex((s) => s.kind === "pick" && s.side === "ally" && s.cellId === 1);
    const t = withTurn(d, steps, i);
    expect(t.turn).toEqual({ kind: "pick", cells: [1, 2], yours: true });
    expect(t.ally.filter((s) => s.picking).map((s) => s.cellId)).toEqual([1, 2]);
    expect(withTurn(d, steps, steps.length)).toMatchObject({ phase: "finalization", turn: null });
    // nextOpen skips filled spots.
    d = setBan(d, "ally", 0, 1);
    d = setBan(d, "ally", 1, 2);
    expect(nextOpen(d, steps)).toBe(2);
  });
});

describe("fearless", () => {
  it("follows the tournament order", () => {
    expect(draftOrder("fearless", "red")).toEqual(draftOrder("tournament", "red"));
  });

  it("a fearless champion leaves the board and can't be taken; the next game keeps roles and adds picks", () => {
    let d = setLocal(setPosition(newManualDraft(), "ally", 0, "middle"), 2);
    d = setChampion(d, "ally", 2, 103);
    d = setChampion(d, "enemy", 5, 238);
    d = setBan(d, "ally", 0, 157);
    d = addFearless(d, 157);
    expect(d.bans.ally[0]?.champion ?? null).toBeNull();
    expect(taken(d).has(157)).toBe(true);
    d = nextGame(d);
    expect(d.fearless).toEqual([157, 103, 238]);
    expect([...d.ally, ...d.enemy].every((s) => s.champion == null)).toBe(true);
    expect(d.ally.find((s) => s.isLocal)?.cellId).toBe(2);
    expect(d.ally[0]!.position).toBe("middle");
    expect(removeFearless(d, 103).fearless).toEqual([157, 238]);
  });
});

describe("taking over a replay", () => {
  it("moves a red-side draft into cells 0–9, keeps locks, drops hovers, and resumes after a missed ban", () => {
    const live = newManualDraft();
    const d = {
      ...live,
      queueId: 420,
      ally: live.enemy.map((s, i) => ({ ...s, isLocal: i === 1, champion: i === 0 ? 103 : null, hover: i === 1 ? 7 : null })),
      enemy: live.ally.map((s, i) => ({ ...s, champion: i === 0 ? 238 : null, position: null })),
      bans: {
        ally: [
          { champion: null, cellId: 5, hover: null },
          { champion: 157, cellId: 6, hover: null },
          { champion: null, cellId: 7, hover: 1 },
        ],
        enemy: [{ champion: 22, cellId: 0, hover: null }],
      },
    };
    const t = fromReplay(d);
    expect(t.side).toBe("red");
    expect(t.mode).toBe("ranked");
    expect(t.draft.ally.map((s) => s.cellId)).toEqual([0, 1, 2, 3, 4]);
    expect(t.draft.ally[0]!.champion).toBe(103);
    expect(t.draft.ally[1]).toMatchObject({ isLocal: true, champion: null, hover: null });
    expect(t.draft.enemy[0]!.champion).toBe(238);
    expect(t.draft.bans.ally.map((b) => b.champion)).toEqual([null, 157]);
    // Two of your bans are done (one missed): continue at your third.
    const steps = draftOrder("ranked", "red");
    expect(steps[resumeStep(t.draft, steps)]).toMatchObject({ kind: "ban", side: "ally", index: 2 });
  });

  it("a spectated or Clash draft becomes a tournament draft with someone as you", () => {
    const d = { ...newManualDraft(), spectating: true, ally: newManualDraft().ally.map((s) => ({ ...s, isLocal: false })) };
    const t = fromReplay(d);
    expect(t).toMatchObject({ mode: "tournament", side: "blue" });
    expect(t.draft.ally.filter((s) => s.isLocal)).toHaveLength(1);
    expect(fromReplay({ ...newManualDraft(), queueId: 700 }).mode).toBe("tournament");
  });
});

