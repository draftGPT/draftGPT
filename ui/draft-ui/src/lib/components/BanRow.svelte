<script lang="ts">
  import type { Champions } from "../champions";
  import type { Ban } from "../types/Ban";
  import ChampionIcon from "./ChampionIcon.svelte";

  type Props = {
    bans: Ban[];
    champions: Champions;
    side: "ally" | "enemy";
    /** Accessible name, e.g. "Your bans" or "Blue side bans". */
    label: string;
    /** Empty ban spots to show; manual input fills them. */
    slots?: number;
    selectedIndex?: number | null;
    onBanClick?: (index: number) => void;
  };
  let { bans, champions, side, label, slots = 0, selectedIndex = null, onBanClick }: Props = $props();

  const spots = $derived(
    Array.from({ length: Math.max(slots, bans.length) }, (_, i) => bans[i] ?? null),
  );
</script>

<div class="bans {side}" role="group" aria-label={label}>
  {#each spots as ban, i (i)}
    {#if onBanClick}
      <button
        type="button"
        class="spot"
        class:selected={selectedIndex === i}
        data-ban={i}
        onclick={() => onBanClick(i)}
        title={ban?.champion != null ? champions.name(ban.champion) : "Set ban"}
      >
        <ChampionIcon champion={ban?.champion ?? null} {champions} size={28} banned={ban?.champion != null} />
      </button>
    {:else}
      <span class="spot" title={ban?.champion != null ? champions.name(ban.champion) : "Missed ban"}>
        <ChampionIcon champion={ban?.champion ?? null} {champions} size={28} banned={ban?.champion != null} />
      </span>
    {/if}
  {/each}
</div>

<style>
  .bans {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    min-height: 30px;
  }
  .enemy {
    justify-content: flex-end;
  }
  .spot {
    padding: 0;
    border: 1px solid transparent;
    border-radius: 7px;
    background: none;
    line-height: 0;
  }
  button.spot {
    cursor: pointer;
  }
  .selected {
    border-color: var(--text);
  }
</style>
