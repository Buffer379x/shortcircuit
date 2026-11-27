<script lang="ts">
  // Overview: inline shortening, key figures, visits against the previous period, links that need
  // attention with the health summary, and the busiest links, sources, and devices.
  import { onMount, tick } from 'svelte';
  import type { CatalogLink, Link, LinkState, TrafficReport } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import type { IconName } from '../lib/icons';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import ShortUrl from '../lib/ui/ShortUrl.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import TimeChart from '../lib/charts/TimeChart.svelte';
  import BarList from '../lib/charts/BarList.svelte';
  import Donut from '../lib/charts/Donut.svelte';
  import Sparkline from '../lib/charts/Sparkline.svelte';
  import { app, shortUrl } from '../lib/app.svelte';
  import { locale, tr } from '../lib/locale.svelte';
  import { linkState } from '../lib/status';
  import { ago, change, num, percent, splitUrl } from '../lib/fmt';
  import { href, route, setParams } from '../lib/router.svelte';
  import { linksChanged, openCreate, overlays } from '../lib/overlays.svelte';
  import { tip } from '../lib/tooltip';
  import {
    DAY_MS, categoryLabel, dayCount, donutCenter, donutItems, isoDay, previousDaily, summaryQuery, type Summary,
  } from './insights';

  type Status = LinkState | 'archived';
  interface AttentionItem {
    id: number;
    host: string;
    slug: string;
    title: string;
    reason: 'unreachable' | 'expiring' | 'limit';
    expires_at: number | null;
    max_visits: number | null;
    visits_used: number;
  }
  interface Dashboard {
    links: Record<Status | 'total' | 'expiring_soon', number>;
    active_domains: number;
    recent_links: { id: number; host: string; slug: string; title: string; created_at: string; state: Status }[];
    monitoring: { enabled: boolean; total: number; healthy: number; attention: number; pending: number; last_checked: number | null };
    /** API v1 addition; absent on older servers, which fall back to the monitoring counts. */
    attention?: AttentionItem[];
  }

  const periods = [7, 30, 90] as const;
  const initial = Number(route.params.get('days'));
  let days = $state<number>(periods.includes(initial as 7) ? initial : 30);
  let compare = $state(true);

  let dashboard = $state.raw<Dashboard>();
  let dashboardError = $state<keyof Messages | null>(null);
  let summary = $state.raw<Summary>();
  let summaryError = $state<keyof Messages | null>(null);
  let previous = $state.raw<number[] | null>(null);
  let traffic = $state.raw<TrafficReport>();
  let details = $state.raw<Record<number, CatalogLink>>({});

  let quick = $state('');
  let quickError = $state<keyof Messages | null>(null);
  let quickResult = $state.raw<Link | null>(null);
  let creating = $state(false);
  let quickInput = $state<HTMLInputElement>();

  let dashboardRequest: AbortController | undefined;
  let summaryRequest: AbortController | undefined;
  const trafficRequest = new AbortController();
  const detailRequest = new AbortController();

  onMount(() => {
    void loadDashboard();
    void loadSummary();
    void loadTraffic();
    return () => {
      dashboardRequest?.abort();
      summaryRequest?.abort();
      trafficRequest.abort();
      detailRequest.abort();
    };
  });

  async function loadDashboard() {
    dashboardRequest?.abort();
    const request = (dashboardRequest = new AbortController());
    dashboardError = null;
    try {
      const result = await app.api<Dashboard>('/overview', 'GET', undefined, 'json', request.signal);
      if (!request.signal.aborted) dashboard = result;
    } catch (error) {
      if (!request.signal.aborted) dashboardError = problemMessage(error);
    }
  }

  async function loadSummary() {
    summaryRequest?.abort();
    const request = (summaryRequest = new AbortController());
    summary = undefined;
    previous = null;
    summaryError = null;
    try {
      const report = await app.api<Summary>(`/analytics/summary?${summaryQuery({ days }, null)}`, 'GET', undefined, 'json', request.signal);
      if (request.signal.aborted) return;
      summary = report;
      void loadDetails(report.top_links.slice(0, 6).map((link) => link.id));
      const values = await previousDaily(app.api, report, request.signal);
      if (!request.signal.aborted) previous = values;
    } catch (error) {
      if (!request.signal.aborted) summaryError = problemMessage(error);
    }
  }

  async function loadTraffic() {
    try {
      traffic = await app.api<TrafficReport>('/analytics/traffic', 'GET', undefined, 'json', trafficRequest.signal);
    } catch {
      /* The unusual-traffic notice is informational; the Analytics page reports its errors. */
    }
  }

  /** Destination and 14-day trend for the top links. Optional: rows stay complete without them. */
  async function loadDetails(ids: number[]) {
    const missing = ids.filter((id) => !details[id]);
    const results = await Promise.allSettled(missing.map((id) =>
      app.api<CatalogLink>(`/links/${id}`, 'GET', undefined, 'json', detailRequest.signal)));
    const found = results.flatMap((result) => (result.status === 'fulfilled' && result.value?.target ? [result.value] : []));
    if (found.length && !detailRequest.signal.aborted) {
      details = { ...details, ...Object.fromEntries(found.map((link) => [link.id, link])) };
    }
  }

  function choosePeriod(next: number) {
    if (next === days) return;
    days = next;
    setParams({ days: next === 30 ? null : String(next) });
    void loadSummary();
  }

  async function shorten(event: SubmitEvent) {
    event.preventDefault();
    if (creating) return;
    quickError = null;
    const target = quick.trim();
    try {
      const url = new URL(target);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error();
    } catch {
      quickError = 'invalidTarget';
      quickInput?.focus();
      return;
    }
    creating = true;
    try {
      const host = app.domains?.hosts[0];
      quickResult = await app.api<Link>('/links', 'POST', {
        target, title: '', description: '', tags: [], ...(host ? { host } : {}),
      });
      quick = '';
      linksChanged();
      void loadDashboard();
    } catch (error) {
      quickError = problemMessage(error);
      await tick();
      quickInput?.focus();
    } finally {
      creating = false;
    }
  }

  const visits = $derived(summary?.daily.map((point) => point.redirect) ?? []);
  const delta = $derived(summary ? change(summary.totals.redirect, summary.previous_redirects) : null);
  const live = $derived(dashboard ? dashboard.links.total - dashboard.links.archived : 0);
  const monitor = $derived(dashboard?.monitoring);

  function relativeDays(seconds: number) {
    const remaining = Math.max(0, Math.ceil((seconds * 1000 - Date.now()) / DAY_MS));
    return new Intl.RelativeTimeFormat(locale.language, { numeric: 'auto' }).format(remaining, 'day');
  }

  function reason(item: AttentionItem): { tone: 'error' | 'warning'; icon: IconName; text: string } {
    if (item.reason === 'unreachable') return { tone: 'error', icon: 'circle-x', text: tr('insReasonUnreachable') };
    if (item.reason === 'limit') {
      return { tone: 'warning', icon: 'gauge', text: tr('insReasonLimit', { used: num(item.visits_used), max: num(item.max_visits) }) };
    }
    return { tone: 'warning', icon: 'clock', text: item.expires_at ? tr('insReasonExpiring', { relative: relativeDays(item.expires_at) }) : tr('stateActive') };
  }

  const attentionCounts = $derived.by(() => {
    const items = dashboard?.attention;
    if (!dashboard) return null;
    if (!items) {
      return { total: dashboard.monitoring.attention + dashboard.links.expiring_soon, unreachable: dashboard.monitoring.attention, ending: dashboard.links.expiring_soon, limit: 0 };
    }
    const count = (kind: AttentionItem['reason']) => items.filter((item) => item.reason === kind).length;
    return { total: items.length, unreachable: count('unreachable'), ending: count('expiring'), limit: count('limit') };
  });

  const healthSplit = $derived(monitor && monitor.enabled && monitor.total
    ? ([['healthy', 'insSumReachable'], ['attention', 'insSumAttention'], ['pending', 'insSumPending']] as const)
      .map(([key, label]) => ({ key, label: tr(label), n: monitor[key] }))
      .filter((part) => part.n > 0)
    : []);

  const sources = $derived(summary?.breakdowns.traffic_source.map((row) => ({ label: categoryLabel(row.category), visits: row.total })) ?? []);
  const devices = $derived(donutItems(summary?.breakdowns.device.map((row) => ({ label: categoryLabel(row.category), total: row.total })) ?? []));
  const topLinks = $derived(summary?.top_links.slice(0, 6) ?? []);
  const hasDetails = $derived(topLinks.some((link) => details[link.id]));

  function recentState(state: Status) {
    return linkState({ archived: state === 'archived', state: state === 'archived' ? 'active' : state, expires_at: null });
  }
</script>

<div class="page">
  <PageHeader title={tr('navOverview')} description={tr('insOverviewDescription')}>
    {#snippet actions()}
      <div class="seg" role="group" aria-label={tr('analyticsRange')}>
        {#each periods as period (period)}
          <button type="button" aria-pressed={days === period} onclick={() => choosePeriod(period)}>{dayCount(period)}</button>
        {/each}
      </div>
    {/snippet}
  </PageHeader>

  <section class="panel quick" aria-label={tr('insShortenLabel')}>
    <form onsubmit={shorten} novalidate aria-busy={creating}>
      <div class="search quick-input">
        <Icon name="link" />
        <input
          bind:this={quickInput}
          class="input"
          type="url"
          bind:value={quick}
          placeholder={tr('insShortenPlaceholder')}
          aria-label={tr('insDestination')}
          aria-invalid={quickError ? 'true' : undefined}
          aria-describedby={quickError ? 'quick-error' : undefined}
          maxlength="2048"
          spellcheck="false"
          autocomplete="off"
        />
      </div>
      <button class="btn" type="submit" disabled={creating}>
        {#if creating}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="zap" />{/if}{tr('insShorten')}
      </button>
      <button class="btn" data-variant="ghost" type="button" onclick={() => openCreate(quick.trim())}>{tr('insMoreOptions')}</button>
    </form>
    {#if quickError}<p class="field-error" id="quick-error"><Icon name="circle-alert" />{tr(quickError)}</p>{/if}
    <div role="status">
      {#if quickResult}
        {@const result = quickResult}
        <div class="quick-result">
          <Icon name="circle-check" />
          <span class="visually-hidden">{tr('linkCreated')}</span>
          <ShortUrl host={result.host} slug={result.slug} />
          <span class="dest truncate">{result.target}</span>
          <div class="row-actions">
            <CopyButton value={shortUrl(result)} variant="secondary" text={tr('copy')} />
            <button class="btn" type="button" data-size="sm" data-variant="ghost" onclick={() => (overlays.qrId = result.id)}><Icon name="qr-code" />{tr('qrCode')}</button>
            <a class="btn" data-size="sm" data-variant="ghost" href={href('links', result.id)}>{tr('insDetails')}</a>
          </div>
        </div>
      {/if}
    </div>
  </section>

  {#if dashboardError}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr(dashboardError)}</p></div>
      <button class="btn" type="button" data-size="sm" onclick={loadDashboard}>{tr('retry')}</button>
    </div>
  {/if}
  {#if summaryError}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr(summaryError)}</p></div>
      <button class="btn" type="button" data-size="sm" onclick={loadSummary}>{tr('retry')}</button>
    </div>
  {/if}

  {#if traffic?.items.length}
    {@const first = traffic.items[0]}
    <div class="notice" data-tone="neutral">
      <Icon name="trending-up" />
      <div>
        <b>{traffic.items.length > 1 ? tr('insUnusualMany', { count: num(traffic.items.length) }) : tr('insUnusualOn', { name: first.title || first.slug })}</b>
        <p>{tr('insUnusualBody', {
          today: num(first.today),
          factor: first.baseline_average > 0 ? new Intl.NumberFormat(locale.language, { maximumFractionDigits: 1 }).format(first.today / first.baseline_average) : '—',
          days: num(traffic.baseline_days),
        })}</p>
      </div>
      <a class="btn" data-size="sm" href={traffic.items.length > 1 ? href('analytics') : href('links', first.id, { tab: 'analytics' })}>
        {traffic.items.length > 1 ? tr('overviewViewAnalytics') : tr('insOpenLink')}
      </a>
    </div>
  {/if}

  <section class="panel kpis" aria-label={tr('insKeyFigures')} aria-busy={!summary || !dashboard}>
    <div class="kpi">
      <span class="kpi-label">{tr('insVisits')}</span>
      <span class="kpi-value num">
        {summary ? num(summary.totals.redirect) : '—'}
        {#if visits.length}<span class="kpi-spark"><Sparkline values={visits.slice(-14)} width={88} height={28} /></span>{/if}
      </span>
      <span class="kpi-foot">
        {#if summary && delta}
          {#if summary.previous_redirects > 0}
            <span class="trend" data-dir={delta.dir}><Icon name={delta.dir === 'down' ? 'trending-down' : 'trending-up'} />{percent(delta.value)}</span>
            {tr('insVsPreviousDays', { days: dayCount(days) })}
          {:else}
            {tr('analyticsNoPriorRedirects')}
          {/if}
        {/if}
      </span>
    </div>
    <div class="kpi">
      <span class="kpi-label">{tr('overviewActiveLinks')}</span>
      <span class="kpi-value num">{dashboard ? num(dashboard.links.active) : '—'}{#if dashboard}<small>{tr('insOfTotal', { total: num(live) })}</small>{/if}</span>
      <span class="kpi-foot">
        {#if dashboard}{tr('insActiveFoot', { scheduled: num(dashboard.links.scheduled), expired: num(dashboard.links.expired), domains: num(dashboard.active_domains) })}{/if}
      </span>
    </div>
    <div class="kpi">
      <span class="kpi-label">{tr('insNeedAttention')}</span>
      <span class="kpi-value num" class:warn={(attentionCounts?.total ?? 0) > 0}>{attentionCounts ? num(attentionCounts.total) : '—'}</span>
      <span class="kpi-foot">
        {#if attentionCounts}{tr('insAttentionFoot', { unreachable: num(attentionCounts.unreachable), ending: num(attentionCounts.ending), limit: num(attentionCounts.limit) })}{/if}
      </span>
    </div>
    <div class="kpi">
      <span class="kpi-label">{tr('insUnmatched')}</span>
      <span class="kpi-value num">{summary ? num(summary.totals.missing) : '—'}</span>
      <span class="kpi-foot">{tr('insUnmatchedFoot')}</span>
    </div>
  </section>

  <div class="grid-main-side">
    <section class="panel">
      <header class="panel-head">
        <div class="panel-title">
          <h2>{tr('insVisits')}</h2>
          <p>
            {tr('insVisitsHint')}{#if summary}{' · '}<time datetime={isoDay(summary.start_day)}>{new Intl.DateTimeFormat(locale.language, { dateStyle: 'medium', timeZone: 'UTC' }).formatRange(new Date(summary.start_day * DAY_MS), new Date(summary.end_day * DAY_MS))}</time>{/if}
          </p>
        </div>
        <span class="spacer"></span>
        <div class="legend">
          <span><i class="line" style:--c="var(--chart-1)"></i>{tr('insThisPeriod')}</span>
          {#if compare && previous}<span><i class="dashed"></i>{tr('insPreviousPeriod')}</span>{/if}
        </div>
        <label class="check compare"><input class="switch" type="checkbox" bind:checked={compare} />{tr('insCompare')}</label>
      </header>
      <div class="panel-body chart-body">
        {#if !summary}
          <div class="skeleton chart-skeleton" aria-hidden="true"></div>
          {#if !summaryError}<span class="visually-hidden" role="status">{tr('loading')}</span>{/if}
        {:else if summary.totals.redirect === 0 && !(previous ?? []).some(Boolean)}
          <p class="muted empty-chart">{tr('analyticsNoRedirects')}</p>
        {:else}
          <TimeChart
            labels={summary.daily.map((point) => isoDay(point.day))}
            caption={tr('insVisitsCaption', { range: dayCount(days) })}
            series={[
              { name: tr('insThisPeriod'), values: visits, color: 'var(--chart-1)', area: true },
              ...(compare && previous ? [{ name: tr('insPreviousPeriod'), values: previous, color: 'var(--chart-previous)', dashed: true }] : []),
            ]}
          />
        {/if}
      </div>
    </section>

    <section class="panel attention-panel">
      <header class="panel-head">
        <div class="panel-title"><h2>{tr('overviewAttention')}</h2><p>{tr('insAttentionHint')}</p></div>
        <span class="spacer"></span>
        <a class="btn" data-variant="ghost" data-size="sm" href={href('health')}>{tr('viewAll')}<Icon name="chevron-right" /></a>
      </header>
      <ul class="attention">
        {#if !dashboard}
          <li class="muted">{dashboardError ? '—' : tr('loading')}</li>
        {:else if dashboard.attention}
          {#each dashboard.attention as item (item.id + item.reason)}
            {@const r = reason(item)}
            <li>
              <span class="att-icon" data-tone={r.tone}><Icon name={r.icon} /></span>
              <a href={href('links', item.id)}><span class="truncate att-title">{item.title || `${item.host}/${item.slug}`}</span><span class="att-reason">{r.text}</span></a>
              <button class="btn" type="button" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('insEditName', { name: item.title || item.slug })} use:tip={tr('insEdit')} onclick={() => (overlays.editId = item.id)}><Icon name="pencil" /></button>
            </li>
          {:else}
            <li class="muted">{tr('insAttentionNone')}</li>
          {/each}
        {:else}
          {#if dashboard.monitoring.attention}
            <li>
              <span class="att-icon" data-tone="error"><Icon name="circle-x" /></span>
              <a href={href('health')}><span class="truncate att-title">{tr('insFallbackUnreachable', { count: num(dashboard.monitoring.attention) })}</span><span class="att-reason">{tr('navHealth')}</span></a>
            </li>
          {/if}
          {#if dashboard.links.expiring_soon}
            <li>
              <span class="att-icon" data-tone="warning"><Icon name="clock" /></span>
              <a href={href('links')}><span class="truncate att-title">{tr('insFallbackExpiring', { count: num(dashboard.links.expiring_soon) })}</span><span class="att-reason">{tr('overviewNextSevenDays')}</span></a>
            </li>
          {/if}
          {#if !dashboard.monitoring.attention && !dashboard.links.expiring_soon}<li class="muted">{tr('insAttentionNone')}</li>{/if}
        {/if}
      </ul>
      {#if monitor}
        <footer class="health-sum">
          {#if !monitor.enabled}
            <p>{tr('overviewMonitorOff')} {tr('opsOffInSettings')} <a href={href('settings', undefined, { section: 'operations' })}>{tr('opsOpenSettings')}</a>.</p>
          {:else if monitor.total === 0}
            <p>{tr('monitorEmpty')}</p>
          {:else}
            <div class="stack-bar" aria-hidden="true">
              {#each healthSplit as part (part.key)}<span style:flex={part.n} data-tone={part.key}></span>{/each}
            </div>
            <p>
              {#each healthSplit as part, i (part.key)}{i ? ', ' : ''}<b class="num">{num(part.n)}</b> {part.label}{/each}
              {#if monitor.last_checked !== null}
                · {tr('overviewLastCheck')} <time datetime={new Date(monitor.last_checked * 1000).toISOString()}>{ago(monitor.last_checked)}</time>
              {/if}
            </p>
          {/if}
        </footer>
      {/if}
    </section>
  </div>

  <div class="grid-main-side">
    <section class="panel">
      <header class="panel-head">
        <div class="panel-title"><h2>{tr('analyticsTopLinks')}</h2><p>{tr('insTopLinksHint', { range: dayCount(days) })}</p></div>
        <span class="spacer"></span>
        <a class="btn" data-variant="ghost" data-size="sm" href={href('analytics')}>{tr('viewAll')}<Icon name="chevron-right" /></a>
      </header>
      {#if topLinks.length}
        <div class="table-wrap">
          <table class="table" data-density="compact">
            <thead>
              <tr>
                <th scope="col">{tr('insColLink')}</th>
                {#if hasDetails}<th scope="col" class="hide-sm">{tr('insColDestination')}</th>{/if}
                <th scope="col" class="r">{tr('insVisits')}</th>
                {#if hasDetails}<th scope="col" class="r hide-sm">{tr('insCol14Days')}</th>{/if}
              </tr>
            </thead>
            <tbody>
              {#each topLinks as link (link.id)}
                {@const detail = details[link.id]}
                <tr>
                  <td><a class="cell-link" href={href('links', link.id)}><span class="truncate">{link.title || link.slug}</span><ShortUrl host={link.host} slug={link.slug} /></a></td>
                  {#if hasDetails}
                    <td class="hide-sm">
                      {#if detail}{@const d = splitUrl(detail.target)}<span class="dest dest-cell"><b>{d.host}</b>{d.rest}</span>{:else}<span class="muted">—</span>{/if}
                    </td>
                  {/if}
                  <td class="r num"><b>{num(link.total)}</b></td>
                  {#if hasDetails}<td class="r hide-sm">{#if detail?.trend?.length}<Sparkline values={detail.trend} />{/if}</td>{/if}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <div class="panel-body"><p class="muted">{summary ? tr('analyticsNoRedirects') : tr('loading')}</p></div>
      {/if}
    </section>

    <section class="panel">
      <header class="panel-head">
        <div class="panel-title"><h2>{tr('insSources')}</h2><p>{tr('insSourcesHint')}</p></div>
        <span class="spacer"></span>
        <a class="btn" data-variant="ghost" data-size="sm" href={href('analytics')}>{tr('viewAll')}<Icon name="chevron-right" /></a>
      </header>
      <div class="panel-body tight">
        {#if sources.length}<BarList items={sources} limit={6} />{:else}<p class="muted">{summary ? tr('analyticsNoRedirects') : tr('loading')}</p>{/if}
      </div>
    </section>
  </div>

  <div class="grid-main-side">
    <section class="panel">
      <header class="panel-head">
        <div class="panel-title"><h2>{tr('insRecent')}</h2></div>
        <span class="spacer"></span>
        <a class="btn" data-variant="ghost" data-size="sm" href={href('links')}>{tr('viewAll')}<Icon name="chevron-right" /></a>
      </header>
      <ul class="recent">
        {#each dashboard?.recent_links ?? [] as link (link.id)}
          <li>
            <a class="cell-link" href={href('links', link.id)}><span class="truncate">{link.title || link.slug}</span><ShortUrl host={link.host} slug={link.slug} /></a>
            <Badge state={recentState(link.state)} quiet />
            <time class="muted when" datetime={link.created_at}>{ago(link.created_at)}</time>
          </li>
        {:else}
          <li class="muted">{dashboard ? tr('noLinks') : tr('loading')}</li>
        {/each}
      </ul>
    </section>
    <section class="panel">
      <header class="panel-head"><div class="panel-title"><h2>{tr('insDevices')}</h2><p>{tr('insDevicesHint')}</p></div></header>
      <div class="panel-body">
        {#if devices.length}<Donut items={devices} center={donutCenter(devices)} />{:else}<p class="muted">{summary ? tr('analyticsNoRedirects') : tr('loading')}</p>{/if}
      </div>
    </section>
  </div>
</div>

<style>
  .quick {
    padding: var(--space-4);
  }
  .quick form {
    display: flex;
    gap: var(--space-2);
  }
  .quick-input {
    flex: 1;
  }
  .quick-input .input {
    height: var(--control-lg);
    font-size: var(--text-md);
  }
  .quick form > :global(.btn) {
    height: var(--control-lg);
  }
  .quick .field-error {
    margin-top: var(--space-2);
  }
  .quick-result {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
    border: 1px solid var(--accent-soft-border);
    border-radius: var(--radius-md);
    background: var(--accent-soft);
    animation: fade-in var(--duration) var(--ease-out);
  }
  .quick-result > :global(svg) {
    flex: none;
    width: var(--icon-md);
    height: var(--icon-md);
    color: var(--success);
  }
  .quick-result .dest {
    flex: 1;
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .kpi {
    display: grid;
    gap: 6px;
    padding: var(--space-5);
    min-width: 0;
  }
  .kpi + .kpi {
    border-left: 1px solid var(--border-subtle);
  }
  .kpi-label {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .kpi-value {
    font-size: var(--text-3xl);
    line-height: var(--leading-3xl);
    font-weight: var(--weight-semibold);
    letter-spacing: var(--tracking-display);
  }
  .kpi-value small {
    margin-left: 6px;
    color: var(--text-muted);
    font-size: var(--text-md);
    font-weight: var(--weight-regular);
    letter-spacing: 0;
  }
  .kpi-value.warn {
    color: var(--warning);
  }
  .kpi-value:has(.kpi-spark) {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .kpi-spark {
    margin-bottom: 4px;
  }
  .kpi-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 6px;
    min-height: var(--leading-xs);
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .page > :global(.grid-main-side) {
    align-items: stretch;
  }
  .compare {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .chart-body {
    padding-top: var(--space-4);
    padding-left: var(--space-3);
  }
  .chart-skeleton {
    height: 240px;
  }
  .empty-chart {
    padding: var(--space-6) 0;
    text-align: center;
  }
  .attention-panel {
    display: flex;
    flex-direction: column;
  }
  .attention {
    flex: 1;
    align-content: start;
  }
  .health-sum {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-5);
    border-top: 1px solid var(--border-subtle);
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .health-sum b {
    color: var(--text-secondary);
    font-weight: var(--weight-medium);
  }
  .stack-bar {
    display: flex;
    gap: 2px;
    height: 6px;
  }
  .stack-bar span {
    border-radius: var(--radius-2xs);
    background: var(--success);
  }
  .stack-bar [data-tone='attention'] {
    background: var(--error);
  }
  .stack-bar [data-tone='pending'] {
    background: var(--border);
  }
  .tight {
    padding: var(--space-3) var(--space-3) var(--space-4);
  }
  .tight > p {
    padding: var(--space-2);
  }
  .attention,
  .recent {
    display: grid;
    padding: 6px;
  }
  .attention li,
  .recent li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }
  .attention li:hover,
  .recent li:hover {
    background: var(--surface-hover);
  }
  .attention a {
    display: grid;
    flex: 1;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }
  .att-title {
    font-weight: var(--weight-medium);
  }
  .att-reason {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .att-icon {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-sm);
    background: var(--warning-soft);
    color: var(--warning);
  }
  .att-icon[data-tone='error'] {
    background: var(--error-soft);
    color: var(--error);
  }
  .att-icon :global(svg) {
    width: var(--icon-md);
    height: var(--icon-md);
  }
  .cell-link {
    display: grid;
    gap: 1px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
    font-weight: var(--weight-medium);
  }
  .cell-link:hover > .truncate {
    color: var(--accent);
  }
  .cell-link :global(.short-url) {
    font-size: var(--text-xs);
    font-weight: var(--weight-regular);
  }
  .dest-cell {
    max-width: 260px;
  }
  .recent .cell-link {
    flex: 1;
  }
  .when {
    width: 9ch;
    font-size: var(--text-xs);
    text-align: right;
  }
  @media (max-width: 1200px) {
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
  @media (max-width: 640px) {
    .quick form {
      flex-wrap: wrap;
    }
    .quick-input {
      flex-basis: 100%;
    }
    .quick form > :global(.btn[type='submit']) {
      flex: 1;
    }
    .kpi {
      padding: var(--space-4);
    }
    .kpi-value {
      font-size: var(--text-2xl);
      line-height: var(--leading-2xl);
    }
    .hide-sm {
      display: none;
    }
    .quick-result {
      flex-wrap: wrap;
    }
  }
</style>
