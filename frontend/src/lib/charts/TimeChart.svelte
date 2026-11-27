<script lang="ts">
  // Time series with an optional comparison period. Pointer and arrow keys move a crosshair;
  // a visually hidden table carries the same numbers for screen readers.
  import { day, num } from '../fmt';
  import { tr } from '../locale.svelte';

  export interface Series { name: string; values: number[]; color: string; dashed?: boolean; area?: boolean }
  let { labels, series, height = 240, caption }: { labels: string[]; series: Series[]; height?: number; caption: string } = $props();

  let width = $state(600);
  let active = $state<number | null>(null);
  const pad = { top: 12, right: 12, bottom: 26, left: 44 };
  const max = $derived(niceMax(Math.max(1, ...series.flatMap((s) => s.values))));
  const iw = $derived(Math.max(10, width - pad.left - pad.right));
  const ih = $derived(height - pad.top - pad.bottom);
  const x = (i: number) => pad.left + (i / Math.max(1, labels.length - 1)) * iw;
  const y = (v: number) => pad.top + ih - (v / max) * ih;
  const ticks = $derived([0, 0.25, 0.5, 0.75, 1].map((t) => t * max));
  const xTicks = $derived(labels.map((_, i) => i).filter((i) => i % Math.ceil(labels.length / (width < 520 ? 4 : 7)) === 0 || i === labels.length - 1));

  // Four gridline steps of 1, 2, 2.5, or 5 times a power of ten.
  function niceMax(v: number) {
    const raw = (v * 1.05) / 4;
    const p = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((m) => m * p).find((s) => s >= raw) ?? 10 * p;
    return step * 4;
  }

  function path(values: number[]) {
    return values.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join('');
  }

  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    const i = Math.round(((e.clientX - r.left - pad.left) / iw) * (labels.length - 1));
    active = Math.min(labels.length - 1, Math.max(0, i));
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowRight') active = Math.min(labels.length - 1, (active ?? -1) + 1);
    else if (e.key === 'ArrowLeft') active = Math.max(0, (active ?? labels.length) - 1);
    else if (e.key === 'Escape') active = null;
    else return;
    e.preventDefault();
  }

  const compactTick = (v: number) => (v >= 1000 ? `${+(v / 1000).toFixed(1)}k` : String(Math.round(v)));
</script>

<div class="chart" bind:clientWidth={width}>
  <!-- The chart is focusable so keyboard users can move the crosshair; the hidden table carries the data. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <svg
    {width}
    {height}
    role="img"
    aria-label={caption}
    tabindex="0"
    onpointermove={move}
    onpointerleave={() => (active = null)}
    onkeydown={key}
    onblur={() => (active = null)}
  >
    <g class="grid">
      {#each ticks as t (t)}<line x1={pad.left} x2={width - pad.right} y1={y(t)} y2={y(t)} />{/each}
    </g>
    <g class="axis">
      {#each ticks as t (t)}<text x={pad.left - 10} y={y(t) + 4} text-anchor="end">{compactTick(t)}</text>{/each}
      {#each xTicks as i (i)}<text x={x(i)} y={height - 6} text-anchor={i === 0 ? 'start' : i === labels.length - 1 ? 'end' : 'middle'}>{day(labels[i])}</text>{/each}
    </g>
    <defs>
      {#each series as s, si (s.name)}
        <linearGradient id="fill-{si}" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stop-color={s.color} stop-opacity=".22" />
          <stop offset="1" stop-color={s.color} stop-opacity="0" />
        </linearGradient>
      {/each}
    </defs>
    {#each series as s, si (s.name)}
      {#if s.area}
        <path d="{path(s.values)}L{x(s.values.length - 1)} {pad.top + ih}L{pad.left} {pad.top + ih}Z" fill="url(#fill-{si})" />
      {/if}
      <path
        d={path(s.values)}
        fill="none"
        stroke={s.color}
        stroke-width={s.dashed ? 1.5 : 2}
        stroke-dasharray={s.dashed ? '4 4' : undefined}
        stroke-linejoin="round"
        stroke-linecap="round"
      />
    {/each}
    {#if active != null}
      <line class="cross" x1={x(active)} x2={x(active)} y1={pad.top} y2={pad.top + ih} />
      {#each series as s (s.name)}
        <circle cx={x(active)} cy={y(s.values[active])} r="4" fill="var(--surface)" stroke={s.color} stroke-width="2" />
      {/each}
    {/if}
  </svg>
  {#if active != null}
    <div class="chart-tip" style:left="{Math.min(x(active) + 12, width - 180)}px" style:top="{pad.top}px">
      <header>{day(labels[active])}</header>
      {#each series as s (s.name)}
        <div class="row"><i class="key" style:--c={s.color} data-dashed={s.dashed || undefined}></i>{s.name}<b>{num(s.values[active])}</b></div>
      {/each}
    </div>
  {/if}
  <div class="visually-hidden"><table>
    <caption>{caption}</caption>
    <thead><tr><th>{tr('chartDate')}</th>{#each series as s (s.name)}<th>{s.name}</th>{/each}</tr></thead>
    <tbody>
      {#each labels as l, i (l)}<tr><td>{l}</td>{#each series as s (s.name)}<td>{s.values[i]}</td>{/each}</tr>{/each}
    </tbody>
  </table></div>
</div>

<style>
  svg {
    display: block;
    touch-action: pan-y;
  }
  svg:focus-visible {
    outline-offset: 4px;
    border-radius: var(--radius-md);
  }
  .cross {
    stroke: var(--border-control);
    stroke-width: 1;
    stroke-dasharray: 2 3;
  }
  .key {
    width: 10px;
    height: 2px;
    border-radius: var(--radius-2xs);
    background: var(--c);
  }
  .key[data-dashed] {
    height: 0;
    background: none;
    border-top: 2px dashed var(--c);
  }
</style>
