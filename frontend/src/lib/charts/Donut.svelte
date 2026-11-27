<script lang="ts">
  // Part-to-whole for at most five categories, with a legend that repeats the numbers.
  import { num } from '../fmt';
  let { items, size = 132, center }: { items: { label: string; visits: number }[]; size?: number; center?: string } = $props();
  const colors = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-5)', 'var(--chart-6)'];
  const total = $derived(items.reduce((s, i) => s + i.visits, 0) || 1);
  const r = 46;
  const c = 2 * Math.PI * r;
  const arcs = $derived.by(() => {
    let acc = 0;
    return items.map((it, i) => {
      const len = (it.visits / total) * c;
      const gap = items.length > 1 ? 2 : 0;
      const a = { ...it, color: colors[i % colors.length], dash: `${Math.max(0, len - gap)} ${c}`, offset: -acc };
      acc += len;
      return a;
    });
  });
</script>

<div class="donut">
  <svg width={size} height={size} viewBox="0 0 120 120" aria-hidden="true">
    <circle cx="60" cy="60" r={r} fill="none" stroke="var(--border-subtle)" stroke-width="12" />
    {#each arcs as a (a.label)}
      <circle cx="60" cy="60" r={r} fill="none" stroke={a.color} stroke-width="12" stroke-dasharray={a.dash} stroke-dashoffset={a.offset} transform="rotate(-90 60 60)" />
    {/each}
    {#if center}<text x="60" y="58" text-anchor="middle" class="c1">{Math.round((items[0].visits / total) * 100)}%</text><text x="60" y="74" text-anchor="middle" class="c2">{center}</text>{/if}
  </svg>
  <ul class="legend-list">
    {#each arcs as a (a.label)}
      <li><i style:background={a.color}></i><span>{a.label}</span><b class="num">{Math.round((a.visits / total) * 100)}%</b><span class="num muted">{num(a.visits)}</span></li>
    {/each}
  </ul>
</div>

<style>
  .donut {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    flex-wrap: wrap;
  }
  .c1 {
    fill: var(--text);
    font-size: 20px; /* design-lint: allow, SVG user units inside a 120-unit viewBox */
    font-weight: 600;
  }
  .c2 {
    fill: var(--text-muted);
    font-size: 10px; /* design-lint: allow, SVG user units inside a 120-unit viewBox */
  }
  .legend-list {
    display: grid;
    gap: 8px;
    flex: 1;
    min-width: 180px;
    font-size: var(--text-sm);
  }
  .legend-list li {
    display: grid;
    grid-template-columns: 10px 1fr auto 6ch;
    align-items: center;
    gap: 10px;
  }
  .legend-list i {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-2xs);
  }
  .legend-list b {
    font-weight: var(--weight-medium);
  }
  .legend-list .muted {
    font-size: var(--text-xs);
    text-align: right;
  }
</style>
