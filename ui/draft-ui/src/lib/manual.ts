// Building a DraftState by hand (the web drafter). The desktop panel gets its
// DraftState from the client instead and never uses this.
//
// Functions return a new draft; they never change their input.

import type { Ban } from "./types/Ban";
import type { DraftState } from "./types/DraftState";
import type { Position } from "./types/Position";
import type { Slot } from "./types/Slot";
import { POSITIONS } from "./format";

export type Side = "ally" | "enemy";

/** Ally cells are 0–4, enemy cells 5–9, like the client in a 5v5 draft. */
export function newManualDraft(localCell = 0): DraftState {
  const slot = (cell: number, side: Side): Slot => ({
    cellId: cell,
    isLocal: side === "ally" && cell === localCell,
    position: side === "ally" ? (POSITIONS[cell] ?? null) : null,
    champion: null,
    hover: null,
    picking: false,
  });
  return {
    phase: "banPick",
    queueId: null,
    spectating: false,
    ally: [0, 1, 2, 3, 4].map((c) => slot(c, "ally")),
    enemy: [5, 6, 7, 8, 9].map((c) => slot(c, "enemy")),
    bans: { ally: [], enemy: [] },
    turn: null,
    timer: { remainingMs: null, totalMs: 0 },
    bench: [],
    offered: [],
    rerollsRemaining: null,
    fearless: [],
  };
}

/** Puts `champion` (or nobody) in a slot. A champion can only be in one slot or ban. */
export function setChampion(
  d: DraftState,
  side: Side,
  cellId: number,
  champion: number | null,
): DraftState {
  const next = champion == null ? d : without(d, champion);
  return {
    ...next,
    [side]: next[side].map((s) => (s.cellId === cellId ? { ...s, champion } : s)),
  };
}

export const MAX_BANS = 5;

/**
 * Sets ban `index` (0–4) of a side; `null` clears it. With `blind` (ranked:
 * both teams ban at once) the other team may ban the same champion.
 */
export function setBan(
  d: DraftState,
  side: Side,
  index: number,
  champion: number | null,
  { blind = false } = {},
): DraftState {
  const next = champion == null ? d : without(d, champion, blind ? other(side) : undefined);
  const ids: (number | null)[] = next.bans[side].map((b) => b.champion);
  while (ids.length <= index) ids.push(null);
  ids[index] = champion;
  // Empty spots at the end drop off; earlier ones stay as empty bans.
  while (ids.length && ids[ids.length - 1] == null) ids.pop();
  const bans: Ban[] = ids.map((c) => ({ champion: c, cellId: null, hover: null }));
  return { ...next, bans: { ...next.bans, [side]: bans } };
}

/** Makes `cellId` (an ally) the local player: recommendations are for them. */
export function setLocal(d: DraftState, cellId: number): DraftState {
  return { ...d, ally: d.ally.map((s) => ({ ...s, isLocal: s.cellId === cellId })) };
}

/**
 * Gives a slot a position. A team has each position once: the slot that held
 * it gets this slot's old position instead (the two swap).
 */
export function setPosition(d: DraftState, side: Side, cellId: number, position: Position | null): DraftState {
  const old = d[side].find((s) => s.cellId === cellId)?.position ?? null;
  return {
    ...d,
    [side]: d[side].map((s) =>
      s.cellId === cellId ? { ...s, position } : position != null && s.position === position ? { ...s, position: old } : s,
    ),
  };
}

const other = (side: Side): Side => (side === "ally" ? "enemy" : "ally");

/** The draft with `champion` removed from every slot and ban (except `keepBans`' bans). */
function without(d: DraftState, champion: number, keepBans?: Side): DraftState {
  const clear = (s: Slot): Slot => (s.champion === champion ? { ...s, champion: null } : s);
  const unban = (b: Ban): Ban => (b.champion === champion ? { ...b, champion: null } : b);
  return {
    ...d,
    ally: d.ally.map(clear),
    enemy: d.enemy.map(clear),
    bans: {
      ally: keepBans === "ally" ? d.bans.ally : d.bans.ally.map(unban),
      enemy: keepBans === "enemy" ? d.bans.enemy : d.bans.enemy.map(unban),
    },
  };
}

/**
 * Champions already picked or banned. `blindBan`: for a ranked ban by that
 * team, where the other team's bans don't count (blind bans can match).
 */
export function taken(d: DraftState, blindBan?: Side): Set<number> {
  const ids = [
    ...d.ally.map((s) => s.champion),
    ...d.enemy.map((s) => s.champion),
    ...(blindBan === "enemy" ? [] : d.bans.ally.map((b) => b.champion)),
    ...(blindBan === "ally" ? [] : d.bans.enemy.map((b) => b.champion)),
    ...d.fearless,
  ];
  return new Set(ids.filter((c): c is number => c != null));
}

// Draft order (manual input). The web drafter walks through these steps; you
// can still click any spot to fix it.

/**
 * `ranked`: all ten bans at once, then picks. `tournament` (Clash): two ban
 * and two pick phases. `fearless`: tournament, plus every champion picked in
 * earlier games of the series is out for both teams.
 */
export type DraftMode = "ranked" | "tournament" | "fearless";
export type MapSide = "blue" | "red";

/** `cellId`: who acts (in tournament draft the captain, slot 1, makes every ban). */
export type Step = { phase: string; group: number; side: Side; cellId: number } & (
  | { kind: "ban"; index: number }
  | { kind: "pick" }
);

/**
 * Every step of a draft in order. Steps with the same `group` happen at the
 * same time (ranked bans) or back to back by one team (two picks in a row).
 */
export function draftOrder(mode: DraftMode, you: MapSide): Step[] {
  const side = (team: MapSide): Side => (team === you ? "ally" : "enemy");
  const steps: Step[] = [];
  let group = 0;
  const base = (s: Side) => (s === "ally" ? 0 : 5);
  // Ranked: every player bans one. Tournament: the captain (first slot) bans.
  const bans = (phase: string, team: MapSide, indices: number[]) => {
    const s = side(team);
    for (const index of indices) {
      const cellId = base(s) + (mode === "ranked" ? index : 0);
      steps.push({ phase, group, side: s, cellId, kind: "ban", index });
    }
    group++;
  };
  // Picks: "B1 R1 R2 B2" → teams in turn, consecutive picks by one team grouped.
  const picks = (phase: string, order: string) => {
    let last = "";
    for (const p of order.split(" ")) {
      const team: MapSide = p[0] === "B" ? "blue" : "red";
      if (last && last !== p[0]) group++;
      last = p[0]!;
      const s = side(team);
      steps.push({ phase, group, side: s, kind: "pick", cellId: base(s) + Number(p.slice(1)) - 1 });
    }
    group++;
  };
  if (mode === "ranked") {
    bans("Ban phase", you, [0, 1, 2, 3, 4]);
    bans("Bans revealed", you === "blue" ? "red" : "blue", [0, 1, 2, 3, 4]);
    picks("Pick phase", "B1 R1 R2 B2 B3 R3 R4 B4 B5 R5");
  } else {
    for (const i of [0, 1, 2]) {
      bans("Ban phase 1", "blue", [i]);
      bans("Ban phase 1", "red", [i]);
    }
    picks("Pick phase 1", "B1 R1 R2 B2 B3 R3");
    for (const i of [3, 4]) {
      bans("Ban phase 2", "red", [i]);
      bans("Ban phase 2", "blue", [i]);
    }
    picks("Pick phase 2", "R4 B4 B5 R5");
  }
  return steps;
}

export function isFilled(d: DraftState, s: Step): boolean {
  return s.kind === "ban"
    ? d.bans[s.side][s.index]?.champion != null
    : d[s.side].find((x) => x.cellId === s.cellId)?.champion != null;
}

/** The first step from `from` on that isn't filled yet; `steps.length` when done. */
export function nextOpen(d: DraftState, steps: Step[], from = 0): number {
  let i = Math.max(from, 0);
  while (i < steps.length && isFilled(d, steps[i]!)) i++;
  return i;
}

/** The draft with the turn and phase of step `i` (finalization once all are done). */
export function withTurn(d: DraftState, steps: Step[], i: number): DraftState {
  const step = steps[i];
  if (!step) return { ...d, phase: "finalization", turn: null };
  const cells = steps.filter((s) => s.group === step.group).map((s) => s.cellId);
  const local = d.ally.find((s) => s.isLocal)?.cellId;
  return {
    ...d,
    phase: "banPick",
    turn: { kind: step.kind, cells, yours: local != null && cells.includes(local) },
    ally: d.ally.map((s) => ({ ...s, picking: step.kind === "pick" && cells.includes(s.cellId) })),
    enemy: d.enemy.map((s) => ({ ...s, picking: step.kind === "pick" && cells.includes(s.cellId) })),
  };
}

// Fearless series (manual input).

/** Marks `champion` as out from an earlier game; it leaves any slot or ban. */
export function addFearless(d: DraftState, champion: number): DraftState {
  if (d.fearless.includes(champion)) return d;
  const next = without(d, champion);
  return { ...next, fearless: [...next.fearless, champion] };
}

export function removeFearless(d: DraftState, champion: number): DraftState {
  return { ...d, fearless: d.fearless.filter((c) => c !== champion) };
}

/** The next game of a Fearless series: this game's picks join the fearless list, the board clears. */
export function nextGame(d: DraftState): DraftState {
  const picked = [...d.ally, ...d.enemy].map((s) => s.champion).filter((c): c is number => c != null);
  const fresh = newManualDraft();
  return {
    ...fresh,
    ally: d.ally.map((s) => ({ ...s, champion: null, hover: null, picking: false })),
    fearless: [...d.fearless, ...picked.filter((c) => !d.fearless.includes(c))],
  };
}
