<script lang="ts">
  // The desktop side panel: the shared DraftView with live data and no input.
  import { DraftView, champions } from "@draftgpt/draft-ui";
  import { demoSource, inTauri, tauriSource, type Source, type Update } from "./source";

  let update = $state<Update | null>(null);
  let receivedAt = $state(Date.now());
  let error = $state<string | null>(null);

  const params = new URLSearchParams(location.search);
  const demo = params.get("demo");
  const source: Source | null = inTauri()
    ? tauriSource()
    : demo
      ? demoSource(demo, Number(params.get("speed") ?? "1") || 1)
      : null;

  source
    ?.start((u) => {
      update = u;
      receivedAt = Date.now();
    })
    .catch((e: unknown) => (error = e instanceof Error ? e.message : String(e)));

  async function toggleFloating() {
    if (!source?.setFloating || !update) return;
    await source.setFloating(!update.floating);
  }
</script>

<main>
  <header>
    <span class="status" class:live={update?.draft != null}>{update?.status ?? (source ? "Starting…" : "Not running in the desktop app")}</span>
    {#if source?.setFloating && update?.canDock}
      <button type="button" onclick={toggleFloating} title="Dock next to the client or float freely">
        {update?.floating ? "Dock" : "Float"}
      </button>
    {/if}
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if update?.draft}
    <DraftView draft={update.draft} recs={update.recs} {champions} layout="panel" {receivedAt} />
  {:else}
    <p class="idle">
      {#if source}
        Waiting for champ select…
      {:else}
        Open this page from the draftGPT app, or add <code>?demo=16_22_draft_ban_and_pick_timeout_autolock</code>.
      {/if}
    </p>
  {/if}
</main>

<style>
  main {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 300px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .status::before {
    content: "●";
    margin-right: 6px;
    color: var(--muted);
  }
  .status.live::before {
    color: var(--good);
  }
  header button {
    padding: 4px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel);
    cursor: pointer;
  }
  .idle {
    color: var(--muted);
    text-align: center;
    margin-top: 40px;
  }
  .error {
    color: var(--bad);
  }
  code {
    font-size: 12px;
  }
</style>
