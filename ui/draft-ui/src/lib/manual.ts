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

/** Sets ban `index` (0–4) of a side; `null` clears it. */
export function setBan(d: DraftState, side: Side, index: number, champion: number | null): DraftState {
  const next = champion == null ? d : without(d, champion);
  const ids: (number | null)[] = next.bans[side].map((b) => b.champion);
  while (ids.length <= index) ids.push(null);
  ids[index] = champion;
  // Empty spots at the end drop off; earlier ones stay as empty bans.
  while (ids.length && ids[ids.length - 1] == null) ids.pop();
  const bans: Ban[] = ids.map((c) => ({ champion: c, cellId: null }));
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

/** The draft with `champion` removed from every slot and ban. */
function without(d: DraftState, champion: number): DraftState {
  const clear = (s: Slot): Slot => (s.champion === champion ? { ...s, champion: null } : s);
  const unban = (b: Ban): Ban => (b.champion === champion ? { ...b, champion: null } : b);
  return {
    ...d,
    ally: d.ally.map(clear),
    enemy: d.enemy.map(clear),
    bans: { ally: d.bans.ally.map(unban), enemy: d.bans.enemy.map(unban) },
  };
}

/** Champions already picked or banned. */
export function taken(d: DraftState): Set<number> {
  const ids = [
    ...d.ally.map((s) => s.champion),
    ...d.enemy.map((s) => s.champion),
    ...d.bans.ally.map((b) => b.champion),
    ...d.bans.enemy.map((b) => b.champion),
  ];
  return new Set(ids.filter((c): c is number => c != null));
}
