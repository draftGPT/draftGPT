<script lang="ts">
  import type { Champions } from "../champions";
  import type { Position } from "../types/Position";
  import type { Slot } from "../types/Slot";
  import SlotRow from "./SlotRow.svelte";

  type Props = {
    title: string;
    /** Shown after the title, e.g. the map side. */
    note?: string;
    slots: Slot[];
    champions: Champions;
    side: "ally" | "enemy";
    selectedCell?: number | null;
    onSlotClick?: (cellId: number) => void;
    onPositionChange?: (cellId: number, position: Position | null) => void;
    onMakeLocal?: (cellId: number) => void;
    /** A "First pick" toggle in the title (manual input); replaces the note. */
    firstPick?: boolean;
    onFirstPick?: (on: boolean) => void;
  };
  let {
    title,
    note,
    slots,
    champions,
    side,
    selectedCell = null,
    onSlotClick,
    onPositionChange,
    onMakeLocal,
    firstPick = false,
    onFirstPick,
  }: Props = $props();
</script>

<section class="team {side}" aria-label={title}>
  <h3>
    {title}{#if onFirstPick}<button
        type="button"
        class="first"
        aria-pressed={firstPick}
        onclick={() => onFirstPick(!firstPick)}
        aria-label={side === "ally" ? "Your team picks first" : "Enemy team picks first"}
        title="The first-pick team is blue side">First pick</button
      >{:else if note}<span class="note">{`· ${note}`}</span>{/if}
  </h3>
  {#if slots.length}
    {#each slots as slot (slot.cellId)}
      <SlotRow
        {slot}
        {champions}
        {side}
        selected={selectedCell === slot.cellId}
        onclick={onSlotClick ? () => onSlotClick(slot.cellId) : undefined}
        onPositionChange={onPositionChange ? (p) => onPositionChange(slot.cellId, p) : undefined}
        onMakeLocal={onMakeLocal ? () => onMakeLocal(slot.cellId) : undefined}
      />
    {/each}
  {:else}
    <p class="none">Not visible in this mode</p>
  {/if}
</section>

<style>
  .team {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 20px;
    margin: 0 0 2px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .ally h3 {
    color: var(--ally);
  }
  .enemy h3 {
    color: var(--enemy);
    justify-content: flex-end;
  }
  .first {
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: pointer;
  }
  .first:hover {
    color: var(--text);
  }
  .first[aria-pressed="true"] {
    background: var(--panel-2);
    border-color: currentColor;
    color: inherit;
  }
  .enemy .first {
    order: -1;
  }
  .note {
    color: var(--muted);
    font-weight: 500;
  }
  .none {
    color: var(--muted);
    font-size: 12px;
    margin: 4px 0;
  }
  .enemy .none {
    text-align: right;
  }
</style>
