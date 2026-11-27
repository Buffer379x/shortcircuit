<script lang="ts">
  // 14-day trend in a table cell. Decorative next to the number, so it is hidden from assistive tech.
  let { values, width = 72, height = 22 }: { values: number[]; width?: number; height?: number } = $props();
  const max = $derived(Math.max(1, ...values));
  const pts = $derived(values.map((v, i) => [(i / Math.max(1, values.length - 1)) * width, height - 2 - (v / max) * (height - 4)]));
  const line = $derived(pts.map((p, i) => `${i ? 'L' : 'M'}${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join(''));
  const flat = $derived(values.every((v) => v === 0));
</script>

<svg class="spark" {width} {height} viewBox="0 0 {width} {height}" aria-hidden="true">
  {#if flat}
    <line x1="0" x2={width} y1={height - 2} y2={height - 2} stroke="var(--border)" stroke-width="1.5" stroke-dasharray="2 3" />
  {:else}
    <path d={line} fill="none" stroke="var(--chart-6)" stroke-width="1.25" stroke-linejoin="round" stroke-linecap="round" />
    <circle cx={pts[pts.length - 1][0]} cy={pts[pts.length - 1][1]} r="2" fill="var(--text)" />
  {/if}
</svg>
