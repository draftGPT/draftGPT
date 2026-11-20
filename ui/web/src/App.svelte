<script lang="ts">
  // The web drafter: the shared DraftView with manual input. It walks through
  // the draft order of the chosen mode (ranked or tournament); click any spot
  // to fix it. Recommendations update as the draft changes.
  import {
    ChampionPicker,
    DraftView,
    MAX_BANS,
    champions,
    draftOrder,
    newManualDraft,
    nextOpen,
    setBan,
    setChampion,
    setLocal,
    setPosition,
    taken,
    withTurn,
    type DraftMode,
    type DraftState,
    type MapSide,
    type Recommendations,
    type Step,
  } from "@draftgpt/draft-ui";
  import { createRecommender, type Recommend } from "./engine";

  type Selection =
    | { kind: "slot"; side: "ally" | "enemy"; cellId: number }
    | { kind: "ban"; side: "ally" | "enemy"; index: number }
    | null;

  let mode = $state<DraftMode>("ranked");
  let side = $state<MapSide>("blue");
  let draft = $state<DraftState>(newManualDraft());
  /** The step of the draft order being filled; past the end when done. */
  let step = $state(0);
  let selection = $state<Selection>(null);
  let recommend = $state<Recommend | null>(null);
  let error = $state<string | null>(null);

  createRecommender().then(
    (r) => (recommend = r),
    (e: unknown) => (error = `Couldn't load the recommender: ${e instanceof Error ? e.message : String(e)}`),
  );

  const steps = $derived(draftOrder(mode, side));
  const current = $derived<Step | undefined>(steps[step]);
  const shown = $derived(withTurn(draft, steps, step));
  const recs = $derived<Recommendations | null>(recommend ? recommend($state.snapshot(shown)) : null);
  const local = $derived(draft.ally.find((s) => s.isLocal));

  const target = (s: Step | undefined): Selection =>
    !s ? null : s.kind === "ban" ? { kind: "ban", side: s.side, index: s.index } : { kind: "slot", side: s.side, cellId: s.cellId };
  const same = (a: Selection, b: Selection) => JSON.stringify(a) === JSON.stringify(b);

  selection = target(draftOrder("ranked", "blue")[0]);

  /** Fills the selected spot; filling the current step moves on to the next open one. */
  function pick(champion: number | null) {
    const s = selection;
    if (!s) return;
    draft =
      s.kind === "slot"
        ? setChampion(draft, s.side, s.cellId, champion)
        : setBan(draft, s.side, s.index, champion, { blind: mode === "ranked" });
    if (champion != null && same(s, target(current))) step = nextOpen(draft, steps, step + 1);
    selection = target(steps[step]);
  }

  /** A missed or skipped ban: the current step stays empty. */
  function skip() {
    step = nextOpen(draft, steps, step + 1);
    selection = target(steps[step]);
  }

  function onRecPick(champion: number, kind: "pick" | "ban") {
    if (current?.kind === kind && current.side === "ally") {
      selection = target(current);
      pick(champion);
    } else if (kind === "pick" && local) {
      draft = setChampion(draft, "ally", local.cellId, champion);
    } else if (kind === "ban") {
      const index = draft.bans.ally.findIndex((b) => b.champion == null);
      const at = index >= 0 ? index : draft.bans.ally.length;
      if (at < MAX_BANS) draft = setBan(draft, "ally", at, champion, { blind: mode === "ranked" });
    }
  }

  /** Mode or side changed: keep what's entered, continue at the first open step. */
  function setOrder(m: DraftMode, s: MapSide) {
    mode = m;
    side = s;
    step = nextOpen(draft, draftOrder(m, s));
    selection = target(draftOrder(m, s)[step]);
  }

  function reset() {
    draft = newManualDraft();
    step = 0;
    selection = target(steps[0]);
  }
</script>

<main>
  <header>
    <h1>draftGPT <span>drafter</span></h1>
    <div class="controls">
      <div class="seg" role="group" aria-label="Draft mode">
        <button type="button" aria-pressed={mode === "ranked"} onclick={() => setOrder("ranked", side)} title="All ten bans at once, then picks">Ranked</button>
        <button type="button" aria-pressed={mode === "tournament"} onclick={() => setOrder("tournament", side)} title="Clash: two ban phases and two pick phases">Tournament</button>
      </div>
      <div class="seg" role="group" aria-label="Your side">
        <button type="button" class="blue" aria-pressed={side === "blue"} onclick={() => setOrder(mode, "blue")}>Blue</button>
        <button type="button" class="red" aria-pressed={side === "red"} onclick={() => setOrder(mode, "red")}>Red</button>
      </div>
      <button type="button" onclick={reset}>Reset</button>
    </div>
  </header>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  <DraftView
    draft={shown}
    {recs}
    phase={current?.phase}
    allySide={side}
    {champions}
    layout="wide"
    show="both"
    banSlots={MAX_BANS}
    {selection}
    onSlotClick={(side, cellId) => (selection = { kind: "slot", side, cellId })}
    onBanClick={(side, index) => (selection = { kind: "ban", side, index })}
    {onRecPick}
    onPositionChange={(side, cellId, p) => (draft = setPosition(draft, side, cellId, p))}
    onMakeLocal={(cellId) => (draft = setLocal(draft, cellId))}
  />

  {#if selection}
    <section class="picking" aria-label="Choose a champion">
      <h2>
        <span>
          {selection.kind === "ban" ? "Ban" : "Pick"} for
          {selection.side === "ally" ? "your team" : "the enemy"}
        </span>
        <span class="actions">
          {#if selection.kind === "ban" && same(selection, target(current))}
            <button type="button" class="skip" onclick={skip} title="The ban was missed or skipped">No ban</button>
          {/if}
          <button type="button" class="close" onclick={() => (selection = null)} aria-label="Close">×</button>
        </span>
      </h2>
      <ChampionPicker {champions} unavailable={taken(draft, mode === "ranked" && selection.kind === "ban" ? selection.side : undefined)} onPick={pick} onClear={() => pick(null)} />
    </section>
  {:else}
    <p class="hint">
      {current ? "Click a slot or a ban to set a champion, a suggestion to take it." : "Draft complete."}
      {#if current}<button type="button" class="link" onclick={() => (selection = target(current))}>Continue</button>{/if}
    </p>
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
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .skip {
    font: inherit;
    font-size: 12px;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    cursor: pointer;
  }
  .link {
    font: inherit;
    background: none;
    border: none;
    color: var(--gold);
    cursor: pointer;
    padding: 0 0 0 4px;
  }
  .seg {
    display: flex;
  }
  .seg button {
    border-radius: 0;
    margin-left: -1px;
  }
  .seg button:first-child {
    border-radius: var(--radius) 0 0 var(--radius);
    margin-left: 0;
  }
  .seg button:last-child {
    border-radius: 0 var(--radius) var(--radius) 0;
  }
  .seg button[aria-pressed="true"] {
    background: var(--panel-2);
    border-color: var(--gold);
    color: var(--text);
    position: relative;
  }
  .seg button[aria-pressed="false"] {
    color: var(--muted);
  }
  .seg .blue[aria-pressed="true"] {
    border-color: var(--ally);
  }
  .seg .red[aria-pressed="true"] {
    border-color: var(--enemy);
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
