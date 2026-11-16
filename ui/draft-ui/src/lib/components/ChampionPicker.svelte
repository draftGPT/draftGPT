<script lang="ts">
  import type { Champions } from "../champions";
  import ChampionIcon from "./ChampionIcon.svelte";

  type Props = {
    champions: Champions;
    /** Picked or banned: shown disabled. */
    unavailable?: Set<number>;
    onPick: (champion: number) => void;
    onClear?: () => void;
  };
  let { champions, unavailable = new Set(), onPick, onClear }: Props = $props();

  let query = $state("");
  const results = $derived(champions.search(query));

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      const first = results.find((c) => !unavailable.has(c.id));
      if (first) onPick(first.id);
    }
  }
</script>

<div class="picker">
  <div class="bar">
    <!-- svelte-ignore a11y_autofocus -->
    <input type="search" placeholder="Search champion…" bind:value={query} {onkeydown} autofocus aria-label="Search champion" />
    {#if onClear}<button type="button" class="clear" onclick={onClear}>Clear</button>{/if}
  </div>
  <div class="grid" role="listbox" aria-label="Champions">
    {#each results as c (c.id)}
      <button
        type="button"
        role="option"
        aria-selected="false"
        class="champ"
        disabled={unavailable.has(c.id)}
        data-champion={c.id}
        aria-label={c.name}
        title={c.name}
        onclick={() => onPick(c.id)}
      >
        <ChampionIcon champion={c.id} {champions} size={44} />
        <span>{c.name}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
  }
  .bar {
    display: flex;
    gap: 6px;
  }
  input {
    flex: 1;
    padding: 8px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    font: inherit;
  }
  .clear {
    padding: 0 12px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel);
    cursor: pointer;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(64px, 1fr));
    gap: 6px;
    overflow-y: auto;
    max-height: 340px;
    padding-right: 4px;
  }
  .champ {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 2px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    cursor: pointer;
    font-size: 11px;
    color: var(--muted);
  }
  .champ span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .champ:hover:not(:disabled) {
    border-color: var(--line);
    color: var(--text);
  }
  .champ:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
</style>
