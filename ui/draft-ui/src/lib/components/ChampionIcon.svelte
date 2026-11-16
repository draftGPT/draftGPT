<script lang="ts">
  import type { Champions } from "../champions";

  type Props = {
    champion: number | null;
    champions: Champions;
    size?: number;
    /** Hovered, not locked: shown faded. */
    hover?: boolean;
    /** Banned: crossed out. */
    banned?: boolean;
  };
  let { champion, champions, size = 40, hover = false, banned = false }: Props = $props();

  let broken = $state(false);
  const src = $derived(champion == null ? undefined : champions.icon(champion));
  const name = $derived(champion == null ? "" : champions.name(champion));
  $effect(() => {
    // A new champion gets a new chance to load.
    void src;
    broken = false;
  });
</script>

<span
  class="icon"
  class:hover
  class:banned
  class:empty={champion == null}
  style:width="{size}px"
  style:height="{size}px"
  title={name}
>
  {#if src && !broken}
    <img {src} alt={name} width={size} height={size} onerror={() => (broken = true)} />
  {:else if champion != null}
    <span class="initials" style:font-size="{Math.round(size / 3)}px">{name.slice(0, 2)}</span>
  {/if}
</span>

<style>
  .icon {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
    background: var(--panel-2);
    border: 1px solid var(--line);
  }
  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .hover img,
  .hover .initials {
    opacity: 0.45;
    filter: grayscale(0.4);
  }
  .banned img {
    filter: grayscale(1);
    opacity: 0.6;
  }
  .banned::after {
    content: "";
    position: absolute;
    inset: 45% -10%;
    background: var(--enemy);
    transform: rotate(-35deg);
    height: 2px;
  }
  .initials {
    color: var(--muted);
    font-weight: 600;
  }
</style>
