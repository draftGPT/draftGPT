<script lang="ts">
  // The web drafter: the shared DraftView with manual input. Click a slot or a
  // ban spot, then a champion; recommendations update as the draft changes.
  import {
    ChampionPicker,
    DraftView,
    MAX_BANS,
    POSITIONS,
    champions,
    newManualDraft,
    positionLabel,
    setBan,
    setChampion,
    setLocal,
    setPosition,
    taken,
    type DraftState,
    type Position,
    type Recommendations,
  } from "@draftgpt/draft-ui";
  import { createRecommender, type Recommend } from "./engine";

  type Selection =
    | { kind: "slot"; side: "ally" | "enemy"; cellId: number }
    | { kind: "ban"; side: "ally" | "enemy"; index: number }
    | null;

  let draft = $state<DraftState>(newManualDraft());
  let selection = $state<Selection>(null);
  let recommend = $state<Recommend | null>(null);
  let error = $state<string | null>(null);

  createRecommender().then(
    (r) => (recommend = r),
    (e: unknown) => (error = `Couldn't load the recommender: ${e instanceof Error ? e.message : String(e)}`),
  );

  const recs = $derived<Recommendations | null>(recommend ? recommend($state.snapshot(draft)) : null);
  const local = $derived(draft.ally.find((s) => s.isLocal));

  function pick(champion: number | null) {
    const s = selection;
    if (!s) return;
    draft = s.kind === "slot" ? setChampion(draft, s.side, s.cellId, champion) : setBan(draft, s.side, s.index, champion);
    selection = next(s);
  }

  /** After filling a spot, move to the next empty one on the same side. */
  function next(s: NonNullable<Selection>): Selection {
    if (s.kind === "slot") {
      const empty = draft[s.side].find((x) => x.champion == null);
      return empty ? { ...s, cellId: empty.cellId } : null;
    }
    const filled = draft.bans[s.side].filter((b) => b.champion != null).length;
    return filled < MAX_BANS ? { ...s, index: Math.min(filled, MAX_BANS - 1) } : null;
  }

  function onRecPick(champion: number, kind: "pick" | "ban") {
    if (kind === "pick" && local) {
      draft = setChampion(draft, "ally", local.cellId, champion);
    } else if (kind === "ban") {
      const index = draft.bans.ally.findIndex((b) => b.champion == null);
      const at = index >= 0 ? index : draft.bans.ally.length;
      if (at < MAX_BANS) draft = setBan(draft, "ally", at, champion);
    }
  }

  function onLocalChange(e: Event) {
    draft = setLocal(draft, Number((e.currentTarget as HTMLSelectElement).value));
  }

  function onPositionChange(e: Event) {
    if (!local) return;
    const v = (e.currentTarget as HTMLSelectElement).value;
    draft = setPosition(draft, local.cellId, v ? (v as Position) : null);
  }

  function reset() {
    draft = newManualDraft();
    selection = null;
  }
</script>

<main>
  <header>
    <h1>draftGPT <span>drafter</span></h1>
    <div class="controls">
      <label>
        You are
        <select value={local?.cellId ?? 0} onchange={onLocalChange} aria-label="Your slot">
          {#each draft.ally as s (s.cellId)}
            <option value={s.cellId}>Slot {s.cellId + 1}</option>
          {/each}
        </select>
      </label>
      <label>
        Role
        <select value={local?.position ?? ""} onchange={onPositionChange} aria-label="Your role">
          <option value="">Any</option>
          {#each POSITIONS as p (p)}
            <option value={p}>{positionLabel(p)}</option>
          {/each}
        </select>
      </label>
      <button type="button" onclick={reset}>Reset</button>
    </div>
  </header>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  <DraftView
    {draft}
    {recs}
    {champions}
    layout="wide"
    show="both"
    banSlots={MAX_BANS}
    {selection}
    onSlotClick={(side, cellId) => (selection = { kind: "slot", side, cellId })}
    onBanClick={(side, index) => (selection = { kind: "ban", side, index })}
    {onRecPick}
  />

  {#if selection}
    <section class="picking" aria-label="Choose a champion">
      <h2>
        {selection.kind === "ban" ? "Ban" : "Pick"} for
        {selection.side === "ally" ? "your team" : "the enemy"}
        <button type="button" class="close" onclick={() => (selection = null)} aria-label="Close">×</button>
      </h2>
      <ChampionPicker {champions} unavailable={taken(draft)} onPick={pick} onClear={() => pick(null)} />
    </section>
  {:else}
    <p class="hint">Click a slot or a ban to set a champion. Click a suggestion to take it.</p>
  {/if}
</main>

<style>
  main {
    max-width: 1100px;
    margin: 0 auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
  }
  h1 span {
    color: var(--muted);
    font-weight: 500;
  }
  .controls {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }
  label {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--muted);
  }
  select,
  .controls button {
    padding: 6px 8px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--text);
    font: inherit;
  }
  .controls button {
    cursor: pointer;
  }
  .picking {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 12px;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 14px;
    display: flex;
    justify-content: space-between;
  }
  .close {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 18px;
    line-height: 1;
    color: var(--muted);
  }
  .hint {
    color: var(--muted);
    margin: 0;
    text-align: center;
  }
  .error {
    color: var(--bad);
    margin: 0;
  }
</style>
