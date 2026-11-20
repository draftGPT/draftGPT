import { describe, expect, it } from "vitest";
import { newManualDraft, setBan, setChampion, setLocal, setPosition, taken } from "./manual";

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
