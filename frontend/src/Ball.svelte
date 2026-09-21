<script lang="ts">
  import type { Snapshot } from './types';
  import type { WorkAreaEdge } from './placement';
  import type { DockTheme } from './theme';

  export let snapshot: Snapshot;
  export let ballKind: string;
  export let ringRatio: number;
  export let pulse: boolean;
  export let theme: DockTheme;
  export let matrixDots: string[];
  export let hideBallBadge: boolean;
  export let docked: boolean;
  export let dockEdge: WorkAreaEdge | null;
  export let onBallPointerEnter: () => void;
  export let onBallPointerLeave: () => void;
  export let onDockedPointerMove: () => void;
  export let onBallPointerDown: (event: PointerEvent) => void;
  export let onBallPointerMove: (event: PointerEvent) => void;
  export let onBallClick: (event: MouseEvent) => void;
  export let markClass: (mark: string) => string;

  $: idleLabel = snapshot.pending_mark || (snapshot.working_count ? '工作中' : '空闲');
</script>

{#snippet ballCount(withSeparator: boolean)}
  <span class="count-work">{snapshot.working_count}</span>
  {#if withSeparator}<span class="count-sep">/</span>{/if}
  <span class="count-total">{snapshot.tracked_count}</span>
{/snippet}

<main
  class="ball-shell"
  class:attention={ballKind === 'wait' || ballKind === 'fail'}
  class:pulse
  class:working={ballKind === 'working'}
  class:docked
  class:dock-left={dockEdge === 'left'}
  class:dock-right={dockEdge === 'right'}
  class:dock-top={dockEdge === 'top'}
  class:dock-bottom={dockEdge === 'bottom'}
  aria-label="OrbCue"
  onpointerenter={onBallPointerEnter}
  onpointerleave={onBallPointerLeave}
  onpointermove={onDockedPointerMove}
>
  <button
    class="ball {ballKind}"
    style="--ratio: {ringRatio}"
    aria-label={`${snapshot.count_label}，${idleLabel}。点击展开或收起面板`}
    title={`${snapshot.count_label}，${idleLabel}。点击展开或收起面板`}
    onpointerdown={onBallPointerDown}
    onpointermove={onBallPointerMove}
    onclick={onBallClick}
  >
    {#if theme === 'glyph'}
      <span class="matrix" aria-hidden="true">
        {#each matrixDots as tone, i (i)}<i class="dot {tone}"></i>{/each}
      </span>
      <span class="count">
        {@render ballCount(true)}
      </span>
    {:else if theme === 'braun'}
      <span class="ball-lcd" aria-hidden="true"></span>
      <span class="ball-ring" aria-hidden="true"></span>
      <span class="count">
        {@render ballCount(false)}
      </span>
    {:else if theme === 'glass'}
      <span class="ball-frost" aria-hidden="true"></span>
      <span class="ball-rim" aria-hidden="true"></span>
      <span class="ball-sheen" aria-hidden="true"></span>
      <span class="ball-arc" aria-hidden="true"></span>
      <span class="count">
        {@render ballCount(true)}
      </span>
    {:else if theme === 'fluent'}
      <span class="count">
        {@render ballCount(true)}
      </span>
      <span class="ball-bar" aria-hidden="true"></span>
    {:else}
      <span class="ball-core" aria-hidden="true"></span>
      <span class="ball-ring" aria-hidden="true"></span>
      <span class="ball-sheen" aria-hidden="true"></span>
      <span class="count">
        {@render ballCount(false)}
      </span>
    {/if}
  </button>
  {#if snapshot.pending_mark && !hideBallBadge}<span class="badge mark-{markClass(snapshot.pending_mark)}" aria-label={snapshot.pending_mark}>{snapshot.pending_mark}</span>{/if}
</main>
