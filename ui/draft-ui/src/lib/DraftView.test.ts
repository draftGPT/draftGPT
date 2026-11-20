import { render, screen, within } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { champions } from "./champions";
import DraftView from "./components/DraftView.svelte";
import type { DraftState } from "./types/DraftState";
import type { Recommendations } from "./types/Recommendations";

const recs: Recommendations = {
  picks: [{ champion: 103, score: 0.55, reasons: [{ kind: "matchup", enemy: 122, delta: 0.03 }] }],
  bans: [{ champion: 157, score: 0.53, reasons: [{ kind: "banRate", banRate: 0.2 }] }],
  position: "middle",
  patch: "16.22",
  mock: true,
};

function draft(turn: DraftState["turn"]): DraftState {
  const slot = (cellId: number, champion: number | null, isLocal = false) => ({
    cellId,
    isLocal,
    position: null,
    champion,
    hover: null,
    picking: false,
  });
  return {
    phase: "banPick",
    queueId: 420,
    spectating: false,
    ally: [slot(0, null, true), slot(1, 266)],
    enemy: [slot(5, 122)],
    bans: { ally: [{ champion: null, cellId: 0, hover: null }], enemy: [] },
    turn,
    timer: { remainingMs: 25_000, totalMs: 30_000 },
    bench: [],
    offered: [],
    rerollsRemaining: null,
  };
}

describe("DraftView", () => {
  it("shows teams, your turn, the countdown and pick suggestions with reasons", () => {
    render(DraftView, {
      draft: draft({ kind: "pick", cells: [0], yours: true }),
      recs,
      champions,
      receivedAt: Date.now(),
    });
    expect(screen.getByText("Your pick")).toBeTruthy();
    expect(screen.getByText("25")).toBeTruthy();
    expect(within(screen.getByRole("region", { name: "Your team" })).getByText("Aatrox")).toBeTruthy();
    const picks = within(screen.getByRole("region", { name: "Suggested picks" }));
    expect(picks.getByText("Ahri")).toBeTruthy();
    expect(picks.getByText("beats Darius (+3.0)")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Suggested bans" })).toBeNull();
    expect(screen.getByText("Mock stats · patch 16.22")).toBeTruthy();
  });

  it("shows ban suggestions during a ban turn and a missed ban as an empty spot", () => {
    render(DraftView, {
      draft: draft({ kind: "ban", cells: [5], yours: false }),
      recs,
      champions,
    });
    expect(screen.getByText("Enemy ban")).toBeTruthy();
    expect(within(screen.getByRole("region", { name: "Suggested bans" })).getByText("Yasuo")).toBeTruthy();
    expect(screen.getByTitle("Missed ban")).toBeTruthy();
  });

  it("shows a teammate's unlocked ban as a hover", () => {
    const d = draft({ kind: "ban", cells: [0, 1], yours: true });
    d.bans.ally = [{ champion: null, cellId: 1, hover: 157 }];
    render(DraftView, { draft: d, recs, champions });
    expect(screen.getByTitle("Banning Yasuo…")).toBeTruthy();
  });
});

