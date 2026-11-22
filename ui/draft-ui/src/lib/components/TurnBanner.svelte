<script lang="ts">
  import { phaseLabel, seconds } from "../format";
  import type { DraftState } from "../types/DraftState";

  type Props = {
    draft: DraftState;
    /** When this draft arrived (`Date.now()`); the countdown runs from here. */
    receivedAt: number;
    /** Shown before the turn, e.g. "Ban phase 2" (manual input knows the phase). */
    phase?: string;
  };
  let { draft, receivedAt, phase }: Props = $props();

  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(id);
  });

  const left = $derived(
    draft.timer.remainingMs == null ? null : draft.timer.remainingMs - (now - receivedAt),
  );
  const turnText = $derived.by(() => {
    const t = draft.turn;
    if (!t) return phaseLabel(draft.phase);
    const what = t.kind === "ban" ? "ban" : "pick";
    if (t.yours) return `Your ${what}`;
    const ally = t.cells.some((c) => draft.ally.some((s) => s.cellId === c));
    if (draft.spectating) return ally ? `Blue ${what}` : `Red ${what}`;
    return ally ? `Ally ${what}` : `Enemy ${what}`;
  });
</script>

<div class="banner" class:yours={draft.turn?.yours}>
  <span class="turn">{#if phase}<span class="phase">{`${phase} · `}</span>{/if}{turnText}</span>
  {#if left != null}
    <span class="timer" class:low={left < 10_000}>{seconds(left)}</span>
  {/if}
</div>

<style>
  .phase {
    color: var(--muted);
    font-weight: 500;
  }
  .banner {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--line);
  }
  .yours {
    border-color: var(--gold);
  }
  .turn {
    font-weight: 600;
  }
  .yours .turn {
    color: var(--gold);
  }
  .timer {
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    font-size: 18px;
  }
  .low {
    color: var(--enemy);
  }
</style>
