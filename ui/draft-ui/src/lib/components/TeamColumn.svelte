<script lang="ts">
  import type { Champions } from "../champions";
  import type { Position } from "../types/Position";
  import type { Slot } from "../types/Slot";
  import SlotRow from "./SlotRow.svelte";

  type Props = {
    title: string;
    slots: Slot[];
    champions: Champions;
    side: "ally" | "enemy";
    selectedCell?: number | null;
    onSlotClick?: (cellId: number) => void;
    onPositionChange?: (cellId: number, position: Position | null) => void;
    onMakeLocal?: (cellId: number) => void;
  };
  let { title, slots, champions, side, selectedCell = null, onSlotClick, onPositionChange, onMakeLocal }: Props =
    $props();
</script>

<section class="team {side}" aria-label={title}>
  <h3>{title}</h3>
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
    text-align: right;
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
