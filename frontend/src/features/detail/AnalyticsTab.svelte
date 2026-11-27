<script lang="ts">
  // Analytics for one link: the last 30 UTC days against the 30 days before, plus breakdowns.
  import { untrack } from 'svelte';
  import { problemMessage } from '../../format';
  import type { Messages } from '../../i18n';
  import Icon from '../../lib/Icon.svelte';
  import BarList from '../../lib/charts/BarList.svelte';
  import Donut from '../../lib/charts/Donut.svelte';
  import TimeChart from '../../lib/charts/TimeChart.svelte';
  import { app } from '../../lib/app.svelte';
  import { change, num, percent } from '../../lib/fmt';
  import { tr } from '../../lib/locale.svelte';
  import { categories, isoDay, series, type Dimension, type Summary } from './analytics';

  let { linkId, slug, summary, error, onretry }: {
    linkId: number;
    slug: string;
    summary: Summary | null;
    error: keyof Messages | null;
    onretry: () => void;
  } = $props();

  let previous = $state.raw<Summary | null>(null);
  let previousError = $state<keyof Messages | null>(null);
  let request = 0;

  $effect(() => {
    const current = summary;
    const id = linkId;
    untrack(() => void loadPrevious(id, current));
  });

  async function loadPrevious(id: number, current: Summary | null) {
    const mine = ++request;
    previous = null;
    previousError = null;
    if (!current) return;
    const query = new URLSearchParams({
      start: isoDay(current.start_day - current.days),
      end: isoDay(current.start_day - 1),
      link: String(id),
    });
    try {
      const result = await app.api<Summary>(`/analytics/summary?${query}`);
      if (mine === request) previous = result;
    } catch (failure) {
      if (mine === request) previousError = problemMessage(failure);
    }
  }

  const now = $derived(summary ? series(summary) : null);
  const before = $derived(previous ? series(previous).values : null);
  const delta = $derived(summary ? change(summary.totals.redirect, summary.previous_redirects) : null);
  const items = (dimension: Dimension) =>
    (summary?.breakdowns[dimension] ?? []).map((b) => ({ label: categories[b.category] ? tr(categories[b.category]) : b.category, visits: b.total }));
  const empty = $derived(summary !== null && summary.totals.redirect === 0);
</script>

{#if error}
  <div class="stack">
    <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(error)}</p>
    <div><button type="button" class="btn" onclick={onretry}><Icon name="refresh-cw" />{tr('retry')}</button></div>
  </div>
{:else if !summary || !now}
  <p class="muted" role="status">{tr('loading')}</p>
{:else}
  <section class="panel kpis" aria-label={tr('detKeyFigures')}>
    <div class="kpi">
      <span>{tr('detVisits30')}</span>
      <b class="num">{num(summary.totals.redirect)}</b>
      {#if delta && summary.previous_redirects}
        <span class="trend" data-dir={delta.dir}>
          <Icon name={delta.dir === 'up' ? 'trending-up' : delta.dir === 'down' ? 'trending-down' : 'arrow-right'} />{percent(delta.value)}
          <span class="visually-hidden">{tr('analyticsRedirectChange')}</span>
        </span>
      {:else}
        <span class="trend" data-dir="flat">{tr('analyticsNoPriorRedirects')}</span>
      {/if}
    </div>
    <div class="kpi"><span>{tr('detPreviousPeriod')}</span><b class="num">{num(summary.previous_redirects)}</b></div>
    <div class="kpi"><span>{tr('analyticsHuman')}</span><b class="num">{num(summary.totals.human)}</b></div>
    <div class="kpi"><span>{tr('analyticsBot')}</span><b class="num">{num(summary.totals.bot)}</b></div>
  </section>
  <section class="panel">
    <header class="panel-head"><div class="panel-title"><h2>{tr('detVisitsTitle')}</h2><p>{tr('detVisitsCompareHint')}</p></div></header>
    <div class="panel-body chart-body">
      <TimeChart
        labels={now.labels}
        series={[
          { name: tr('detThisPeriod'), values: now.values, color: 'var(--chart-1)', area: true },
          ...(before && before.length === now.values.length ? [{ name: tr('detPreviousPeriod'), values: before, color: 'var(--chart-previous)', dashed: true }] : []),
        ]}
        caption={tr('detVisitsCaption', { slug })}
      />
      {#if previousError}<p class="field-hint">{tr('detPreviousUnavailable')}</p>{/if}
    </div>
  </section>
  {#if empty}
    <p class="muted">{tr('analyticsNoRedirects')}</p>
  {:else}
    <div class="grid-2">
      <section class="panel"><header class="panel-head"><div class="panel-title"><h2>{tr('analyticsTrafficSource')}</h2></div></header><div class="panel-body tight"><BarList items={items('traffic_source')} /></div></section>
      <section class="panel"><header class="panel-head"><div class="panel-title"><h2>{tr('analyticsDevice')}</h2></div></header><div class="panel-body"><Donut items={items('device').slice(0, 5)} /></div></section>
      <section class="panel"><header class="panel-head"><div class="panel-title"><h2>{tr('analyticsBrowser')}</h2></div></header><div class="panel-body tight"><BarList items={items('browser')} color="var(--chart-2)" /></div></section>
      <section class="panel"><header class="panel-head"><div class="panel-title"><h2>{tr('analyticsOs')}</h2></div></header><div class="panel-body tight"><BarList items={items('os')} color="var(--chart-3)" /></div></section>
    </div>
  {/if}
{/if}

<style>
  .kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .kpi {
    display: grid;
    align-content: start;
    gap: 4px;
    padding: var(--space-4) var(--space-5);
  }
  .kpi + .kpi {
    border-left: 1px solid var(--border-subtle);
  }
  .kpi > span:first-child {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .kpi b {
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
    font-weight: var(--weight-semibold);
  }
  .chart-body {
    padding-left: var(--space-3);
  }
  .tight {
    padding: var(--space-3);
  }
  @media (max-width: 640px) {
    .kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .kpi:nth-child(3) {
      border-left: 0;
    }
    .kpi:nth-child(n + 3) {
      border-top: 1px solid var(--border-subtle);
    }
  }
</style>
