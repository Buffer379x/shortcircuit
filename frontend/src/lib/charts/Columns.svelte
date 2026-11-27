<script lang="ts">
  // Vertical bars for a fixed set of buckets, for example visits per hour of day.
  import { num } from '../fmt';
  let { values, labels, caption, height = 120, highlight }: { values: number[]; labels: string[]; caption: string; height?: number; highlight?: number } = $props();
  const max = $derived(Math.max(1, ...values));
  let hover = $state<number | null>(null);
</script>

<figure class="cols" style:--h="{height}px" aria-label={caption}>
  {#each values as v, i (i)}
    <div class="col" onpointerenter={() => (hover = i)} onpointerleave={() => (hover = null)} role="presentation">
      <span class="fill" class:hl={i === highlight || i === hover} style:height="{Math.max(2, (v / max) * 100)}%"></span>
      {#if hover === i}<span class="tip num">{labels[i]}: {num(v)}</span>{/if}
    </div>
  {/each}
  <figcaption class="visually-hidden">{caption}: {values.map((v, i) => `${labels[i]} ${v}`).join(', ')}</figcaption>
</figure>
<div class="cols-axis" aria-hidden="true"><span>{labels[0]}</span><span>{labels[Math.floor(labels.length / 2)]}</span><span>{labels[labels.length - 1]}</span></div>

<style>
  .cols {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    align-items: end;
    gap: 3px;
    height: var(--h);
    margin: 0;
  }
  .col {
    position: relative;
    display: flex;
    align-items: flex-end;
    height: 100%;
  }
  .fill {
    width: 100%;
    border-radius: var(--radius-2xs) var(--radius-2xs) 0 0;
    background: color-mix(in srgb, var(--chart-1) 38%, transparent);
  }
  .fill.hl {
    background: var(--chart-1);
  }
  .tip {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    padding: 3px 7px;
    border-radius: var(--radius-xs);
    background: var(--surface-raised);
    box-shadow: var(--shadow-popover);
    font-size: var(--text-xs);
    white-space: nowrap;
    z-index: 2;
  }
  .cols-axis {
    display: flex;
    justify-content: space-between;
    margin-top: 6px;
    color: var(--chart-axis);
    font-size: var(--text-2xs);
  }
</style>
