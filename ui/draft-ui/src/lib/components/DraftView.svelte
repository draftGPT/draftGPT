<script lang="ts">
  // The whole draft screen. Both UIs render this: the desktop panel with
  // `layout="panel"` and live data, the web drafter with `layout="wide"` and
  // click handlers for manual input. Props only; no data loading here.
  import type { Champions } from "../champions";
  import type { DraftState } from "../types/DraftState";
  import type { Position } from "../types/Position";
  import type { Recommendations } from "../types/Recommendations";
  import BanRow from "./BanRow.svelte";
  import RecommendationList from "./RecommendationList.svelte";
  import TeamColumn from "./TeamColumn.svelte";
  import TurnBanner from "./TurnBanner.svelte";

  type Selection =
    | { kind: "slot"; side: "ally" | "enemy"; cellId: number }
    | { kind: "ban"; side: "ally" | "enemy"; index: number }
    | null;

  type Props = {
    draft: DraftState;
    recs: Recommendations | null;
    champions: Champions;
    /** `panel`: one narrow column (desktop side panel). `wide`: teams left and right. */
    layout?: "panel" | "wide";
    /** When the draft arrived, for the countdown. */
    receivedAt?: number;
    /** `auto`: bans during ban turns, picks otherwise. `both`: always both. */
    show?: "auto" | "both";
    /** Empty ban spots per team to show (manual input). */
    banSlots?: number;
    selection?: Selection;
    onSlotClick?: (side: "ally" | "enemy", cellId: number) => void;
    onBanClick?: (side: "ally" | "enemy", index: number) => void;
    onRecPick?: (champion: number, kind: "pick" | "ban") => void;
    /** Role selectors on every slot (manual input). */
    onPositionChange?: (side: "ally" | "enemy", cellId: number, position: Position | null) => void;
    /** Phase name for the turn banner (manual input). */
    phase?: string;
    /** Your team's map side, shown in the team titles (manual input). */
    allySide?: "blue" | "red";
    /** "You" toggles on your team's slots (manual input). */
    onMakeLocal?: (cellId: number) => void;
  };
  let {
    draft,
    recs,
    champions,
    layout = "panel",
    receivedAt = Date.now(),
    show = "auto",
    banSlots = 0,
    selection = null,
    onSlotClick,
    onBanClick,
    onRecPick,
    onPositionChange,
    onMakeLocal,
    phase,
    allySide,
  }: Props = $props();

  const banning = $derived(draft.turn?.kind === "ban" || (draft.phase === "planning" && !draft.turn));
  const showBans = $derived(show === "both" || banning);
  const showPicks = $derived(show === "both" || !banning);
  const enemySide = $derived(allySide === "blue" ? "Red side" : allySide === "red" ? "Blue side" : undefined);
  const sel = (side: "ally" | "enemy") =>
    selection?.kind === "slot" && selection.side === side ? selection.cellId : null;
  const selBan = (side: "ally" | "enemy") =>
    selection?.kind === "ban" && selection.side === side ? selection.index : null;
</script>

<div class="draft {layout}">
  <div class="top">
    <TurnBanner {draft} {receivedAt} {phase} />
    <div class="bans-line">
      <BanRow
        bans={draft.bans.ally}
        {champions}
        side="ally"
        label={draft.spectating ? "Blue side bans" : "Your bans"}
        slots={banSlots}
        selectedIndex={selBan("ally")}
        onBanClick={onBanClick ? (i) => onBanClick("ally", i) : undefined}
      />
      <BanRow
        bans={draft.bans.enemy}
        {champions}
        side="enemy"
        label={draft.spectating ? "Red side bans" : "Enemy bans"}
        slots={banSlots}
        selectedIndex={selBan("enemy")}
        onBanClick={onBanClick ? (i) => onBanClick("enemy", i) : undefined}
      />
    </div>
  </div>

  <TeamColumn
    title={draft.spectating ? "Blue side" : "Your team"}
    note={allySide === "blue" ? "Blue side" : allySide === "red" ? "Red side" : undefined}
    slots={draft.ally}
    {champions}
    side="ally"
    selectedCell={sel("ally")}
    onSlotClick={onSlotClick ? (c) => onSlotClick("ally", c) : undefined}
    onPositionChange={onPositionChange ? (c, p) => onPositionChange("ally", c, p) : undefined}
    {onMakeLocal}
  />

  <div class="middle">
    {#if recs}
      {#if showPicks}
        <RecommendationList
          title="Suggested picks"
          items={recs.picks}
          {champions}
          onPick={onRecPick ? (c) => onRecPick(c, "pick") : undefined}
        />
      {/if}
      {#if showBans}
        <RecommendationList
          title="Suggested bans"
          items={recs.bans}
          {champions}
          showScore={false}
          onPick={onRecPick ? (c) => onRecPick(c, "ban") : undefined}
        />
      {/if}
      {#if recs.mock}
        <p class="mock" title="Stats are generated placeholders until the real stats store exists">
          Mock stats · patch {recs.patch}
        </p>
      {/if}
    {/if}
  </div>

  <TeamColumn
    title={draft.spectating ? "Red side" : "Enemy team"}
    note={enemySide}
    slots={draft.enemy}
    {champions}
    side="enemy"
    selectedCell={sel("enemy")}
    onSlotClick={onSlotClick ? (c) => onSlotClick("enemy", c) : undefined}
    onPositionChange={onPositionChange ? (c, p) => onPositionChange("enemy", c, p) : undefined}
  />

  {#if draft.bench.length || draft.offered.length}
    <div class="extra">
      {#if draft.offered.length}
        <span class="label">Offered</span>
        <span class="ids">{draft.offered.map((c) => champions.name(c)).join(", ")}</span>
      {/if}
      {#if draft.bench.length}
        <span class="label">Bench</span>
        <span class="ids">{draft.bench.map((c) => champions.name(c)).join(", ")}</span>
      {/if}
      {#if draft.rerollsRemaining != null}
        <span class="label">Rerolls</span><span>{draft.rerollsRemaining}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .draft {
    display: grid;
    gap: 10px;
  }
  .panel {
    grid-template-columns: 1fr;
  }
  .wide {
    grid-template-columns: minmax(200px, 1fr) minmax(260px, 1.3fr) minmax(200px, 1fr);
    grid-template-areas:
      "top top top"
      "ally middle enemy"
      "extra extra extra";
    align-items: start;
  }
  .wide .top {
    grid-area: top;
  }
  .wide .middle {
    grid-area: middle;
  }
  .wide .extra {
    grid-area: extra;
  }
  .wide :global(.team.ally) {
    grid-area: ally;
  }
  .wide :global(.team.enemy) {
    grid-area: enemy;
  }
  .top {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bans-line {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .middle {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .mock {
    margin: 0;
    font-size: 11px;
    color: var(--muted);
    text-align: center;
  }
  .extra {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .label {
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .ids {
    color: var(--text);
  }
  @media (max-width: 760px) {
    .wide {
      grid-template-columns: 1fr;
      grid-template-areas: "top" "ally" "enemy" "middle" "extra";
    }
  }
</style>
