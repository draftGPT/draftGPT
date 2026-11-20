import { describe, expect, it } from "vitest";
import {
  draftOrder,
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
    expect(d.bans.ally).toEqual([]);
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

