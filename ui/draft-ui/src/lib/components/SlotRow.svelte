<script lang="ts">
  import type { Champions } from "../champions";
  import { positionLabel } from "../format";
  import type { Slot } from "../types/Slot";
  import ChampionIcon from "./ChampionIcon.svelte";

  type Props = {
    slot: Slot;
    champions: Champions;
    side: "ally" | "enemy";
    /** Highlighted as the slot being edited (manual input). */
    selected?: boolean;
    /** Makes the row a button (manual input). */
    onclick?: () => void;
  };
  let { slot, champions, side, selected = false, onclick }: Props = $props();

  const shown = $derived(slot.champion ?? slot.hover);
  const label = $derived(
    slot.champion != null
      ? champions.name(slot.champion)
      : slot.hover != null
        ? champions.name(slot.hover)
        : slot.picking
          ? "Picking…"
          : "",
  );
</script>

{#snippet content()}
  <ChampionIcon champion={shown} {champions} hover={slot.champion == null && slot.hover != null} />
  <span class="text">
    <span class="name" class:muted={slot.champion == null}>{label || "—"}</span>
    <span class="meta">
      {#if slot.isLocal}<span class="you">You</span>{/if}
      {positionLabel(slot.position)}
    </span>
  </span>
{/snippet}

{#if onclick}
  <button
    type="button"
    class="slot {side} clickable"
    class:local={slot.isLocal}
    class:picking={slot.picking}
    class:selected
    data-cell={slot.cellId}
    {onclick}
  >
    {@render content()}
  </button>
{:else}
  <div class="slot {side}" class:local={slot.isLocal} class:picking={slot.picking} data-cell={slot.cellId}>
    {@render content()}
  </div>
{/if}

<style>
  .slot {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    border-radius: var(--radius);
    border: 1px solid transparent;
    background: var(--panel);
    text-align: left;
  }
  .enemy {
    flex-direction: row-reverse;
    text-align: right;
  }
  .clickable {
    cursor: pointer;
  }
  .clickable:hover {
    border-color: var(--line);
  }
  .local {
    border-color: color-mix(in srgb, var(--gold) 60%, transparent);
  }
  .picking {
    box-shadow: inset 3px 0 0 var(--ally);
  }
  .enemy.picking {
    box-shadow: inset -3px 0 0 var(--enemy);
  }
  .selected {
    border-color: var(--text);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .muted {
    color: var(--muted);
    font-weight: 500;
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
  }
  .you {
    color: var(--gold);
    font-weight: 600;
    margin-right: 4px;
  }
</style>
