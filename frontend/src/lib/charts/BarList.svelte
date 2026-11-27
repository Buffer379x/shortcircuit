<script lang="ts">
  // Ranked horizontal bars: label, bar, value, and share. Readable without color.
  import { num } from '../fmt';
  let { items, color = 'var(--chart-6)', limit = 6, onselect }:
    { items: { label: string; visits: number; mono?: boolean; href?: string }[]; color?: string; limit?: number; onselect?: (label: string) => void } = $props();
  const total = $derived(items.reduce((s, i) => s + i.visits, 0) || 1);
  const max = $derived(Math.max(1, ...items.map((i) => i.visits)));
  const shown = $derived(items.slice(0, limit));
</script>

<ol class="bars">
  {#each shown as item (item.label)}
    <li>
      <div class="bar" style:--w="{(item.visits / max) * 100}%" style:--c={color}></div>
      {#if item.href}
        <a class="label" class:mono={item.mono} href={item.href}>{item.label}</a>
      {:else}
        <span class="label" class:mono={item.mono}>{item.label}</span>
      {/if}
      <span class="value num">{num(item.visits)}</span>
      <span class="share num">{Math.round((item.visits / total) * 100)}%</span>
    </li>
  {/each}
</ol>

<style>
  .bars {
    display: grid;
    gap: 2px;
  }
  li {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 3.5ch;
    align-items: center;
    gap: var(--space-3);
    height: 30px;
    padding: 0 10px;
    font-size: var(--text-sm);
  }
  .bar {
    position: absolute;
    inset: 3px auto 3px 0;
    width: var(--w);
    min-width: 2px;
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--c) 13%, transparent);
  }
  li:first-child .bar {
    background: color-mix(in srgb, var(--c) 22%, transparent);
    box-shadow: inset 2px 0 0 var(--c);
  }
  .label {
    position: relative;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    text-decoration: none;
  }
  a.label:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .label.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
  .value {
    position: relative;
    font-weight: var(--weight-medium);
  }
  .share {
    position: relative;
    color: var(--text-secondary);
    font-size: var(--text-xs);
    text-align: right;
  }
</style>
