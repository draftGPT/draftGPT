<script lang="ts">
  import type { Champions } from "../champions";
  import { percent, reasonText, reasonTone } from "../format";
  import type { Recommendation } from "../types/Recommendation";
  import ChampionIcon from "./ChampionIcon.svelte";

  type Props = {
    title: string;
    items: Recommendation[];
    champions: Champions;
    /** How many to show. */
    limit?: number;
    /** Show the score as a win rate (picks); ban scores only rank. */
    showScore?: boolean;
    /** Makes entries buttons (manual input: take the suggestion). */
    onPick?: (champion: number) => void;
  };
  let { title, items, champions, limit = 5, showScore = true, onPick }: Props = $props();
</script>

<section class="recs" aria-label={title}>
  <h3>{title}</h3>
  {#if items.length === 0}
    <p class="none">Nothing to suggest yet</p>
  {/if}
  <ol>
    {#each items.slice(0, limit) as rec (rec.champion)}
      <li>
        {#snippet content()}
          <ChampionIcon champion={rec.champion} {champions} size={34} />
          <span class="body">
            <span class="head">
              <span class="name">{champions.name(rec.champion)}</span>
              {#if showScore}<span class="score">{percent(rec.score)}</span>{/if}
            </span>
            <span class="reasons">
              {#each rec.reasons as r, i (i)}
                <span class={reasonTone(r)}>{reasonText(r, champions)}</span>
              {/each}
            </span>
          </span>
        {/snippet}
        {#if onPick}
          <button type="button" class="rec clickable" data-champion={rec.champion} onclick={() => onPick(rec.champion)}>
            {@render content()}
          </button>
        {:else}
          <div class="rec" data-champion={rec.champion}>{@render content()}</div>
        {/if}
      </li>
    {/each}
  </ol>
</section>

<style>
  .recs {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .rec {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    width: 100%;
    padding: 6px 8px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid transparent;
    text-align: left;
  }
  .clickable {
    cursor: pointer;
  }
  .clickable:hover {
    border-color: var(--line);
  }
  .body {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .name {
    font-weight: 600;
  }
  .score {
    font-variant-numeric: tabular-nums;
    color: var(--gold);
    font-weight: 600;
  }
  .reasons {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .good {
    color: var(--good);
  }
  .bad {
    color: var(--bad);
  }
  .none {
    color: var(--muted);
    font-size: 12px;
    margin: 0;
  }
</style>
