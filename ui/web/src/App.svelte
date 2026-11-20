<script lang="ts">
  // The web drafter: the shared DraftView with manual input. It walks through
  // the draft order of the chosen mode (ranked, tournament, optionally a
  // Fearless series); click
  // any spot to fix it. A recording can be replayed step by step and taken
  // over at any step. Recommendations update as the draft changes.
  import {
    ChampionPicker,
    addFearless,
    nextGame,
    removeFearless,
    DraftView,
    MAX_BANS,
    champions,
    draftOrder,
    fromReplay,
    newManualDraft,
    resumeStep,
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
    type Replay,
    type Step,
  } from "@draftgpt/draft-ui";
  import { createRecommender, fetchRecording, listRecordings, replayRecording, type Recommend } from "./engine";
  import ReplayBar from "./ReplayBar.svelte";

  type Selection =
    | { kind: "slot"; side: "ally" | "enemy"; cellId: number }
    | { kind: "ban"; side: "ally" | "enemy"; index: number }
    | { kind: "fearless" }
    | null;

  let mode = $state<DraftMode>("ranked");
  let side = $state<MapSide>("blue");
  /** Tournament only: a Fearless series. */
  let fearless = $state(false);
  let draft = $state<DraftState>(newManualDraft());
  /** The step of the draft order being filled; past the end when done. */
  let step = $state(0);
  /** Game of the Fearless series. */
  let game = $state(1);
  let selection = $state<Selection>(null);
  let recommend = $state<Recommend | null>(null);
  let error = $state<string | null>(null);

  /** Replay: the recording being stepped through, and the step shown. */
  let replay = $state<{ name: string; data: Replay } | null>(null);
  let replayIndex = $state(0);
  let chooser = $state(false);
  let recordings = $state<string[]>([]);

  createRecommender().then(
    (r) => (recommend = r),
    (e: unknown) => (error = `Couldn't load the recommender: ${e instanceof Error ? e.message : String(e)}`),
  );

  const steps = $derived(draftOrder(mode, side));
  const current = $derived<Step | undefined>(steps[step]);
  const shown = $derived(withTurn(draft, steps, step));
  // A replayed step, timer stopped (it would count down from when you got there).
  const replayed = $derived.by<DraftState | null>(() => {
    const d = replay?.data.steps[replayIndex]?.draft;
    return d ? { ...d, timer: { ...d.timer, remainingMs: null } } : null;
  });
  const view = $derived(replayed ?? shown);
  const recs = $derived<Recommendations | null>(recommend ? recommend($state.snapshot(view)) : null);
  const local = $derived(draft.ally.find((s) => s.isLocal));

  const target = (s: Step | undefined): Selection =>
    !s ? null : s.kind === "ban" ? { kind: "ban", side: s.side, index: s.index } : { kind: "slot", side: s.side, cellId: s.cellId };
  const same = (a: Selection, b: Selection) => JSON.stringify(a) === JSON.stringify(b);

  selection = target(draftOrder("ranked", "blue")[0]);

  /** Fills the selected spot; filling the current step moves on to the next open one. */
  function pick(champion: number | null) {
    const s = selection;
    if (!s) return;
    if (s.kind === "fearless") {
      // Stays open: earlier games usually add several at once.
      if (champion != null) draft = addFearless(draft, champion);
      return;
    }
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

  /** Fearless off: the earlier games are gone. */
  function setFearless(on: boolean) {
    fearless = on;
    if (!on) {
      draft = { ...draft, fearless: [] };
      game = 1;
    }
  }

  /** Mode or side changed: keep what's entered, continue at the first open step. */
  function setOrder(m: DraftMode, s: MapSide) {
    if (m !== "tournament") setFearless(false);
    mode = m;
    side = s;
    step = nextOpen(draft, draftOrder(m, s));
    selection = target(draftOrder(m, s)[step]);
  }

  /** Fearless: this game's picks become unavailable, the next game starts. */
  function startNextGame() {
    draft = nextGame(draft);
    game++;
    step = 0;
    selection = target(steps[0]);
  }

  async function openChooser() {
    chooser = !chooser;
    if (chooser && !recordings.length) recordings = await listRecordings().catch(() => []);
  }

  async function load(name: string, text: Promise<string>) {
    try {
      const data = await replayRecording(await text);
      if (!data.steps.length) throw new Error("no champ select in this recording");
      replay = { name, data };
      replayIndex = 0;
      chooser = false;
      error = null;
    } catch (e) {
      error = `Couldn't replay ${name}: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  function onFile(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    if (file) void load(file.name, file.text());
    e.currentTarget.value = "";
  }

  /** Continue the replayed draft by hand from the step shown. */
  function takeOver() {
    const d = replay?.data.steps[replayIndex]?.draft;
    if (!d) return;
    const t = fromReplay(d);
    mode = t.mode;
    side = t.side;
    fearless = false;
    draft = t.draft;
    game = 1;
    step = resumeStep(t.draft, draftOrder(t.mode, t.side));
    selection = target(draftOrder(t.mode, t.side)[step]);
    replay = null;
  }

  function reset() {
    draft = newManualDraft();
    game = 1;
    step = 0;
    selection = target(steps[0]);
  }
</script>

<main>
  <header>
    <h1>draftGPT <span>drafter</span></h1>
    <div class="controls">
      <button type="button" aria-expanded={chooser} onclick={openChooser} title="Step through a recorded draft">Replay…</button>
      {#if !replay}
      <div class="seg" role="group" aria-label="Draft mode">
        <button type="button" aria-pressed={mode === "ranked"} onclick={() => setOrder("ranked", side)} title="All ten bans at once, then picks">Ranked</button>
        <button type="button" aria-pressed={mode === "tournament"} onclick={() => setOrder("tournament", side)} title="Clash: two ban phases and two pick phases">Tournament</button>
      </div>
      <button
        type="button"
        class="toggle"
        aria-pressed={fearless}
        disabled={mode !== "tournament"}
        onclick={() => setFearless(!fearless)}
        title={mode === "tournament" ? "A series: champions picked in earlier games are out for both teams" : "Fearless is a tournament draft series"}
      >Fearless</button>
      <!-- Hidden, not removed: the header doesn't shift when Fearless toggles. -->
      <button type="button" class:hidden={!fearless} disabled={!fearless} onclick={startNextGame} title="This game's picks become unavailable">Next game</button>
      <button type="button" onclick={reset}>Reset</button>
      {/if}
    </div>
  </header>

  {#if chooser}
    <section class="chooser" aria-label="Choose a recording">
      <label>
        Recording
        <select
          aria-label="Recording"
          onchange={(e) => {
            const n = e.currentTarget.value;
            if (n) void load(n, fetchRecording(n));
          }}
        >
          <option value="">Choose…</option>
          {#each recordings as r (r)}<option value={r}>{r}</option>{/each}
        </select>
      </label>
      <label class="file">
        or open a file (.jsonl from a capture)
        <input type="file" accept=".jsonl" onchange={onFile} aria-label="Open a recording file" />
      </label>
    </section>
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if replay}
    <ReplayBar
      replay={replay.data}
      name={replay.name}
      index={replayIndex}
      onIndex={(i) => (replayIndex = i)}
      onTakeOver={takeOver}
      onClose={() => (replay = null)}
    />
    <DraftView draft={view} {recs} {champions} layout="wide" show="both" />
  {:else}
  <DraftView
    draft={shown}
    {recs}
    phase={fearless ? `Game ${game}${current ? ` · ${current.phase}` : ""}` : current?.phase}
    onFearlessAdd={fearless ? () => (selection = { kind: "fearless" }) : undefined}
    onFearlessRemove={fearless ? (c) => (draft = removeFearless(draft, c)) : undefined}
    allySide={side}
    onAllySideChange={(s) => setOrder(mode, s)}
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
          {#if selection.kind === "fearless"}
            Picked in earlier games (out for both teams)
          {:else}
            {selection.kind === "ban" ? "Ban" : "Pick"} for
            {selection.side === "ally" ? "your team" : "the enemy"}
          {/if}
        </span>
        <span class="actions">
          {#if selection.kind === "ban" && same(selection, target(current))}
            <button type="button" class="skip" onclick={skip} title="The ban was missed or skipped">No ban</button>
          {/if}
          <button type="button" class="close" onclick={() => (selection = null)} aria-label="Close">×</button>
        </span>
      </h2>
      {#if selection.kind === "fearless"}
        <ChampionPicker {champions} unavailable={new Set(draft.fearless)} onPick={pick} />
      {:else}
        <ChampionPicker
          {champions}
          unavailable={taken(draft, mode === "ranked" && selection.kind === "ban" ? selection.side : undefined)}
          onPick={pick}
          onClear={() => pick(null)}
        />
      {/if}
    </section>
  {:else}
    <p class="hint">
      {current ? "Click a slot or a ban to set a champion, a suggestion to take it." : "Draft complete."}
      {#if current}<button type="button" class="link" onclick={() => (selection = target(current))}>Continue</button>{/if}
    </p>
  {/if}
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
  .chooser {
    display: flex;
    flex-wrap: wrap;
    gap: 12px 24px;
    align-items: center;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--line);
  }
  .chooser label {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
  }
  .chooser select {
    padding: 5px 8px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    font: inherit;
    max-width: 100%;
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
  .toggle[aria-pressed="true"] {
    background: var(--panel-2);
    border-color: var(--gold);
  }
  .toggle[aria-pressed="false"] {
    color: var(--muted);
  }
  .controls button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .controls .hidden {
    visibility: hidden;
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
