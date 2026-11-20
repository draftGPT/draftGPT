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
    /** A "First pick" checkbox in the title (manual input); replaces the note. */
    firstPick?: boolean;
    onFirstPick?: (checked: boolean) => void;
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
    {title}{#if onFirstPick}<label class="first"
        ><input
          type="checkbox"
          checked={firstPick}
          onchange={(e) => onFirstPick(e.currentTarget.checked)}
          aria-label={side === "ally" ? "Your team picks first" : "Enemy team picks first"}
        />First pick</label
      >{:else if note}<span class="note">{` · ${note}`}</span>{/if}
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
  .first {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: 8px;
    color: var(--muted);
    font-weight: 500;
    cursor: pointer;
  }
  .first input {
    margin: 0;
    accent-color: var(--gold);
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
