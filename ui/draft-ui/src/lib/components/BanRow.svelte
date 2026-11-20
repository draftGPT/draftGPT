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

  function title(ban: Ban | null): string {
    if (ban?.champion != null) return champions.name(ban.champion);
    if (ban?.hover != null) return `Banning ${champions.name(ban.hover)}…`;
    return ban ? "Missed ban" : "";
  }

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
      <span class="spot" title={title(ban)}>
        <ChampionIcon
          champion={ban?.champion ?? ban?.hover ?? null}
          {champions}
          size={28}
          banned={ban?.champion != null}
          hover={ban?.champion == null && ban?.hover != null}
        />
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
