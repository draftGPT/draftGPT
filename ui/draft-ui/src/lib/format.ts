import type { Champions } from "./champions";
import type { Phase } from "./types/Phase";
import type { Position } from "./types/Position";
import type { Reason } from "./types/Reason";

export const POSITIONS: readonly Position[] = ["top", "jungle", "middle", "bottom", "utility"];

const POSITION_LABELS: Record<Position, string> = {
  top: "Top",
  jungle: "Jungle",
  middle: "Mid",
  bottom: "Bot",
  utility: "Support",
};

const PHASE_LABELS: Record<Phase, string> = {
  planning: "Planning",
  banPick: "Ban & pick",
  finalization: "Finalization",
  gameStarting: "Game starting",
  unknown: "Champ select",
};

export function positionLabel(p: Position | null | undefined): string {
  return p ? POSITION_LABELS[p] : "";
}

export function phaseLabel(p: Phase): string {
  return PHASE_LABELS[p];
}

/** `0.5234` → `52.3%`. */
export function percent(x: number): string {
  return `${(x * 100).toFixed(1)}%`;
}

/** `0.0213` → `+2.1`, in percentage points. */
export function points(delta: number): string {
  const v = (delta * 100).toFixed(1);
  return delta >= 0 ? `+${v}` : v;
}

/** Seconds, rounded up, never negative: `"23"`. */
export function seconds(ms: number): string {
  return String(Math.max(0, Math.ceil(ms / 1000)));
}

export function reasonText(r: Reason, champions: Champions): string {
  switch (r.kind) {
    case "winRate":
      return `${percent(r.winRate)} win rate ${positionLabel(r.position)}`;
    case "matchup":
      return r.delta >= 0
        ? `beats ${champions.name(r.enemy)} (${points(r.delta)})`
        : `weak into ${champions.name(r.enemy)} (${points(r.delta)})`;
    case "synergy":
      return r.delta >= 0
        ? `pairs with ${champions.name(r.ally)} (${points(r.delta)})`
        : `clashes with ${champions.name(r.ally)} (${points(r.delta)})`;
    case "banRate":
      return `banned ${percent(r.banRate)}`;
    case "threat":
      return `beats your ${champions.name(r.ally)} (${points(r.delta)})`;
  }
}

/** How a reason reads: for the champion, against it, or plain context. */
export function reasonTone(r: Reason): "good" | "bad" | "neutral" {
  switch (r.kind) {
    case "matchup":
    case "synergy":
      return r.delta >= 0 ? "good" : "bad";
    case "threat":
      return "good";
    case "winRate":
    case "banRate":
      return "neutral";
  }
}
