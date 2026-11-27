<script lang="ts">
  // The route a request takes: the short link, the checks that can stop it, the ordered rules, and the
  // default destination, drawn as circuit traces. The lit trace is the default destination, or the
  // route a simulated request took. The server keeps no traffic per rule, so no shares are shown.
  import Icon from '../lib/Icon.svelte';
  import { splitUrl } from '../lib/fmt';
  import { tr } from '../lib/locale.svelte';

  let { host, slug, target, checks, rules, lit = 0 }: {
    host: string;
    slug: string;
    target: string;
    checks: string[];
    rules: { label: string; target: string }[];
    /** Rule number to highlight, or 0 for the default destination. */
    lit?: number;
  } = $props();

  const routes = $derived([
    ...rules.map((r, i) => ({ ...r, n: i + 1 })),
    { label: '', target, n: 0 },
  ]);
  const rowH = 56;
  const h = $derived(routes.length * rowH);
</script>

<div class="route" style:--rows={routes.length}>
  <div class="node source">
    <span class="kicker">{tr('detRouteRequest')}</span>
    <span class="mono strong truncate">/{slug}</span>
    <span class="muted mono small truncate">{host}</span>
  </div>

  <span class="wire-in" aria-hidden="true"></span>

  <div class="node checks">
    <span class="kicker">{tr('detRouteChecks')}</span>
    {#if checks.length}
      {#each checks as c (c)}<span class="check-chip"><Icon name="shield-check" />{c}</span>{/each}
    {:else}
      <span class="muted small">{tr('detRouteNoChecks')}</span>
    {/if}
  </div>

  <svg class="wires" viewBox="0 0 100 {h}" preserveAspectRatio="none" aria-hidden="true">
    {#each routes as r, i (r.n)}
      {@const y = i * rowH + rowH / 2}
      <path d="M0 {h / 2} C50 {h / 2} 50 {y} 100 {y}" class:lit={r.n === lit} vector-effect="non-scaling-stroke" />
    {/each}
  </svg>

  <ol class="targets">
    {#each routes as r (r.n)}
      {@const d = splitUrl(r.target)}
      <li class:lit={r.n === lit}>
        <span class="pad" aria-hidden="true"></span>
        <div class="t-main">
          <span class="t-label">{#if r.n}<b>{tr('detRuleNumber', { n: r.n })}</b> {r.label}{:else}<b>{tr('simulationFallback')}</b>{/if}</span>
          <span class="dest"><b>{d.host}</b>{d.rest}</span>
        </div>
      </li>
    {/each}
  </ol>
</div>

<style>
  .route {
    display: grid;
    grid-template-columns: minmax(120px, 170px) 32px minmax(110px, 170px) minmax(48px, 96px) minmax(0, 1fr);
    align-items: center;
    min-height: calc(var(--rows) * 56px);
  }
  .wire-in {
    grid-column: 2;
    grid-row: 1;
    height: 2px;
    background: var(--accent);
  }
  .wires {
    grid-column: 4;
    grid-row: 1;
    width: 100%;
    height: calc(var(--rows) * 56px);
  }
  .wires path {
    fill: none;
    stroke: var(--border);
    stroke-width: 1.5;
  }
  .wires path.lit {
    stroke: var(--accent);
    stroke-width: 2;
  }
  .node {
    position: relative;
    display: grid;
    gap: 2px;
    min-width: 0;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
  }
  .source {
    grid-column: 1;
    grid-row: 1;
    border-color: var(--accent-soft-border);
  }
  .checks {
    grid-column: 3;
    grid-row: 1;
    justify-self: start;
    gap: 4px;
  }
  .kicker {
    color: var(--text-muted);
    font-size: var(--text-2xs);
  }
  .strong {
    color: var(--text);
    font-weight: var(--weight-semibold);
    font-size: var(--text-sm);
  }
  .small {
    font-size: var(--text-xs);
  }
  .check-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-xs);
    color: var(--text-secondary);
  }
  .check-chip :global(svg) {
    width: 12px;
    height: 12px;
    color: var(--success);
  }
  .targets {
    grid-column: 5;
    grid-row: 1;
    display: grid;
    grid-auto-rows: 56px;
    min-width: 0;
  }
  .targets li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .pad {
    flex: none;
    width: 10px;
    height: 10px;
    margin-left: -5px;
    border: 2px solid var(--border-control);
    border-radius: 50%;
    background: var(--surface);
  }
  .lit .pad {
    border-color: var(--accent);
    background: var(--accent);
  }
  .t-main {
    display: grid;
    min-width: 0;
    flex: 1;
  }
  .t-label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .t-label b {
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  @media (max-width: 760px) {
    .route {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-3);
    }
    .wires,
    .wire-in {
      display: none;
    }
    .source,
    .checks,
    .targets {
      grid-column: 1;
      grid-row: auto;
    }
    .checks {
      justify-self: stretch;
    }
    .targets {
      grid-auto-rows: auto;
      gap: 10px;
      border-left: 2px solid var(--border);
      padding-left: 10px;
    }
    .pad {
      margin-left: -17px;
    }
  }
</style>
