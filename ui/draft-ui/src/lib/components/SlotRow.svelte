<script lang="ts">
  import type { Champions } from "../champions";
  import { POSITIONS, positionLabel } from "../format";
  import type { Position } from "../types/Position";
  import type { Slot } from "../types/Slot";
  import ChampionIcon from "./ChampionIcon.svelte";

  type Props = {
    slot: Slot;
    champions: Champions;
    side: "ally" | "enemy";
    /** Highlighted as the slot being edited (manual input). */
    selected?: boolean;
    /** Makes the champion a button (manual input). */
    onclick?: () => void;
    /** Shows a role selector instead of the role text (manual input). */
    onPositionChange?: (position: Position | null) => void;
    /** Shows a "You" toggle on the row (manual input, your team). */
    onMakeLocal?: () => void;
  };
  let { slot, champions, side, selected = false, onclick, onPositionChange, onMakeLocal }: Props = $props();

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
  const who = $derived(`${side === "ally" ? "ally" : "enemy"} ${(slot.cellId % 5) + 1}`);
  const editable = $derived(onPositionChange != null || onMakeLocal != null);
</script>

{#snippet champion()}
  <ChampionIcon champion={shown} {champions} hover={slot.champion == null && slot.hover != null} />
  <span class="text">
    <span class="name" class:muted={slot.champion == null}>{label || (onclick ? "Choose…" : "—")}</span>
    {#if !editable}
      <span class="meta">
        {#if slot.isLocal}<span class="you">You</span>{/if}
        {positionLabel(slot.position)}
      </span>
    {/if}
  </span>
{/snippet}

<div
  class="slot {side}"
  class:local={slot.isLocal}
  class:picking={slot.picking}
  class:selected
  data-cell={slot.cellId}
>
  {#if onclick}
    <button type="button" class="champion clickable" {onclick} aria-label="Champion, {who}">
      {@render champion()}
    </button>
  {:else}
    <div class="champion">{@render champion()}</div>
  {/if}

  {#if editable}
    <div class="tags">
      {#if onPositionChange}
        <select
          class="role"
          class:unset={slot.position == null}
          value={slot.position ?? ""}
          aria-label="Role, {who}"
          onchange={(e) => {
            const v = e.currentTarget.value;
            onPositionChange(v ? (v as Position) : null);
          }}
        >
          <option value="">{side === "ally" ? "Any role" : "Role?"}</option>
          {#each POSITIONS as p (p)}
            <option value={p}>{positionLabel(p)}</option>
          {/each}
        </select>
      {/if}
      {#if onMakeLocal}
        <button
          type="button"
          class="me"
          class:on={slot.isLocal}
          aria-pressed={slot.isLocal}
          aria-label="You are {who}"
          title={slot.isLocal ? "Suggestions are for this slot" : "Make this slot you"}
          onclick={onMakeLocal}
        >
          You
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .slot {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px;
    border-radius: var(--radius);
    border: 1px solid transparent;
    background: var(--panel);
  }
  .enemy {
    flex-direction: row-reverse;
  }
  .champion {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .enemy .champion {
    flex-direction: row-reverse;
    text-align: right;
  }
  .clickable {
    cursor: pointer;
  }
  .slot:has(.clickable:hover) {
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
  .selected,
  .slot.selected:has(.clickable:hover) {
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
  .tags {
    display: flex;
    gap: 4px;
    align-items: center;
    flex-shrink: 0;
  }
  .enemy .tags {
    flex-direction: row-reverse;
  }
  .role,
  .me {
    font: inherit;
    font-size: 12px;
    padding: 3px 6px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    cursor: pointer;
  }
  .role.unset {
    color: var(--muted);
  }
  .me {
    color: var(--muted);
  }
  .me.on {
    color: var(--bg);
    background: var(--gold);
    border-color: var(--gold);
    font-weight: 600;
  }
</style>
