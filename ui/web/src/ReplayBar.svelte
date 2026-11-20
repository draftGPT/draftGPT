<script lang="ts">
  // Stepping through a replayed recording: one step per change in the draft.
  import type { Replay } from "@draftgpt/draft-ui";

  type Props = {
    replay: Replay;
    name: string;
    index: number;
    onIndex: (i: number) => void;
    onTakeOver: () => void;
    onClose: () => void;
  };
  let { replay, name, index, onIndex, onTakeOver, onClose }: Props = $props();

  const last = $derived(replay.steps.length - 1);
  const go = (i: number) => onIndex(Math.min(Math.max(i, 0), last));
  const time = (ms: number) => {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };

  function onkeydown(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    if (el?.closest("input, select, textarea")) return;
    if (e.key === "ArrowLeft") go(index - 1);
    else if (e.key === "ArrowRight") go(index + 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(last);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<section class="replay" aria-label="Replay">
  <div class="title">
    <span class="name" title={name}>{name}</span>
    <span class="meta">
      {replay.clientVersion ?? replay.label} · step {index + 1} / {replay.steps.length} · {time(replay.steps[index]?.t ?? 0)}
    </span>
  </div>
  <div class="controls">
    <button type="button" onclick={() => go(0)} disabled={index === 0} aria-label="First step">⏮</button>
    <button type="button" onclick={() => go(index - 1)} disabled={index === 0} aria-label="Previous step">◀</button>
    <input
      type="range"
      min="0"
      max={last}
      value={index}
      oninput={(e) => go(Number(e.currentTarget.value))}
      aria-label="Step"
    />
    <button type="button" onclick={() => go(index + 1)} disabled={index === last} aria-label="Next step">▶</button>
    <button type="button" onclick={() => go(last)} disabled={index === last} aria-label="Last step">⏭</button>
  </div>
  <div class="actions">
    <button type="button" class="primary" onclick={onTakeOver} title="Continue this draft by hand from this step">Take over from here</button>
    <button type="button" onclick={onClose}>Close replay</button>
  </div>
</section>

<style>
  .replay {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 8px 16px;
    align-items: center;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--line);
  }
  .title {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .controls,
  .actions {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  input[type="range"] {
    width: 220px;
    accent-color: var(--gold);
  }
  button {
    padding: 5px 9px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .primary {
    border-color: var(--gold);
  }
  @media (max-width: 860px) {
    .replay {
      grid-template-columns: 1fr;
    }
    input[type="range"] {
      flex: 1;
      width: auto;
    }
  }
</style>
