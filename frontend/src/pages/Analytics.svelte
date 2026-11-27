<script lang="ts">
  // Analytics: the redirect report for a preset or custom UTC period, optionally for one domain,
  // with the comparison period, all breakdowns, the unusual-traffic indicator, and the versioned
  // report download. Collection and retention settings live in Settings.
  import { onMount, untrack } from 'svelte';
  import { ApiError, type TrafficReport } from '../api';
  import { downloadFile, problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import TimeChart from '../lib/charts/TimeChart.svelte';
  import BarList from '../lib/charts/BarList.svelte';
  import Donut from '../lib/charts/Donut.svelte';
  import { app } from '../lib/app.svelte';
  import { locale, tr } from '../lib/locale.svelte';
  import { change, num, percent } from '../lib/fmt';
  import { href, route, setParams } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import {
    DAY_MS, categoryLabel, dayCount, donutCenter, donutItems, isoDay, previousDaily, reportCsv, share, summaryQuery,
    type Dimension, type Period, type Summary,
  } from './insights';

  const presets = [7, 30, 90, 365] as const;
  const dimensions: [Dimension, keyof Messages][] = [
    ['traffic_source', 'insSources'], ['device', 'insDevices'], ['browser', 'insBrowsers'], ['os', 'insSystems'],
  ];
  const isoPattern = /^\d{4}-\d{2}-\d{2}$/;

  // The period and domain come from the URL, so reload and Back keep the report.
  const params = route.params;
  const urlStart = params.get('start') ?? '';
  const urlEnd = params.get('end') ?? '';
  const urlDays = Number(params.get('days'));
  const customFromUrl = isoPattern.test(urlStart) && isoPattern.test(urlEnd);
  const initialDays = presets.includes(urlDays as 7) ? urlDays : 30;
  let range = $state<number | 'custom'>(customFromUrl ? 'custom' : initialDays);
  let period = $state<Period>(customFromUrl ? { start: urlStart, end: urlEnd } : { days: initialDays });
  let host = $state<string | null>(params.get('host') || null);
  let startDate = $state(urlStart);
  let endDate = $state(urlEnd);
  let invalidDates = $state(false);
  let startInput = $state<HTMLInputElement>();

  let summary = $state.raw<Summary | null>(null);
  let previous = $state.raw<number[] | null>(null);
  let loading = $state(true);
  let summaryError = $state<keyof Messages | null>(null);
  let traffic = $state.raw<TrafficReport | null>(null);
  let trafficError = $state<keyof Messages | null>(null);
  let aggregateDays = $state<number | null>(null);
  let exporting = $state(false);

  let summaryRequest: AbortController | undefined;
  let trafficRequest: AbortController | undefined;
  const settingsRequest = new AbortController();

  onMount(() => {
    void loadSummary();
    void loadTraffic();
    void loadRetention();
    return () => {
      summaryRequest?.abort();
      trafficRequest?.abort();
      settingsRequest.abort();
    };
  });

  // A domain link in the Domains ranking changes the hash; follow it without a remount.
  $effect(() => {
    const next = route.params.get('host') || null;
    untrack(() => {
      if (next !== host) {
        host = next;
        void loadSummary();
        void loadTraffic();
      }
    });
  });

  function syncUrl() {
    setParams({
      days: range === 'custom' || range === 30 ? null : String(range),
      start: 'start' in period ? period.start : null,
      end: 'end' in period ? period.end : null,
      host,
    });
  }

  async function loadSummary() {
    summaryRequest?.abort();
    const request = (summaryRequest = new AbortController());
    loading = true;
    summaryError = null;
    summary = null;
    previous = null;
    try {
      const result = await app.api<Summary>(`/analytics/summary?${summaryQuery(period, host)}`, 'GET', undefined, 'json', request.signal);
      if (request.signal.aborted) return;
      summary = result;
      loading = false;
      const values = await previousDaily(app.api, result, request.signal);
      if (!request.signal.aborted) previous = values;
    } catch (error) {
      if (request.signal.aborted) return;
      if (error instanceof ApiError && error.code === 'invalid_period') {
        range = 'custom';
        invalidDates = true;
      } else summaryError = problemMessage(error);
    } finally {
      if (!request.signal.aborted) loading = false;
    }
  }

  async function loadTraffic() {
    trafficRequest?.abort();
    const request = (trafficRequest = new AbortController());
    trafficError = null;
    try {
      const query = host ? `?host=${encodeURIComponent(host)}` : '';
      const result = await app.api<TrafficReport>(`/analytics/traffic${query}`, 'GET', undefined, 'json', request.signal);
      if (!request.signal.aborted) traffic = result;
    } catch (error) {
      if (!request.signal.aborted) trafficError = problemMessage(error);
    }
  }

  /** Only read here, for the notice about comparisons that reach past the kept daily totals. */
  async function loadRetention() {
    try {
      const settings = await app.api<{ aggregate_days: number }>('/analytics/settings', 'GET', undefined, 'json', settingsRequest.signal);
      aggregateDays = settings.aggregate_days;
    } catch {
      /* The notice is optional; Settings reports errors with these settings. */
    }
  }

  function choosePreset(next: number) {
    invalidDates = false;
    range = next;
    period = { days: next };
    syncUrl();
    void loadSummary();
  }

  function chooseCustom() {
    if (range === 'custom') return;
    const today = summary?.today ?? Math.floor(Date.now() / DAY_MS);
    startDate = isoDay(summary?.start_day ?? today - 29);
    endDate = isoDay(summary?.end_day ?? today);
    invalidDates = false;
    range = 'custom';
  }

  function applyDates(event: SubmitEvent) {
    event.preventDefault();
    const start = Date.parse(`${startDate}T00:00:00Z`);
    const end = Date.parse(`${endDate}T00:00:00Z`);
    const length = (end - start) / DAY_MS + 1;
    const today = summary?.today ?? Math.floor(Date.now() / DAY_MS);
    invalidDates = !(length >= 1 && length <= 365 && end <= today * DAY_MS);
    if (invalidDates) {
      startInput?.focus();
      return;
    }
    period = { start: startDate, end: endDate };
    syncUrl();
    void loadSummary();
  }

  function chooseHost(next: string | null) {
    host = next;
    syncUrl();
    void loadSummary();
    void loadTraffic();
  }

  async function exportReport(format: 'json' | 'csv') {
    const report = summary;
    if (exporting || !report) return;
    exporting = true;
    const query = new URLSearchParams({ start: isoDay(report.start_day), end: isoDay(report.end_day) });
    if (report.host) query.set('host', report.host);
    try {
      const result = await app.api<{ version: number; granularity: string; report: Summary }>(`/analytics/export?${query}`);
      const name = `shortcircuit-analytics-v${result.version}.${format}`;
      const file = format === 'csv'
        ? new Blob([reportCsv(result)], { type: 'text/csv;charset=utf-8' })
        : new Blob([JSON.stringify(result, null, 2)], { type: 'application/json' });
      downloadFile(file, name);
      toast(tr('insExported'), { body: name });
    } catch (error) {
      toast(tr(problemMessage(error)), { tone: 'error' });
    } finally {
      exporting = false;
    }
  }

  const hosts = $derived([...new Set([...(app.domains?.hosts ?? []), ...(host ? [host] : [])])]);
  const totals = $derived(summary?.totals);
  const delta = $derived(summary ? change(summary.totals.redirect, summary.previous_redirects) : null);
  const periodLength = $derived(summary ? dayCount(summary.end_day - summary.start_day + 1) : '');
  const dateRange = $derived(summary
    ? new Intl.DateTimeFormat(locale.language, { dateStyle: 'medium', timeZone: 'UTC' })
      .formatRange(new Date(summary.start_day * DAY_MS), new Date(summary.end_day * DAY_MS))
    : '');
  const visits = $derived(summary?.daily.map((point) => point.redirect) ?? []);
  const dailyDesc = $derived([...(summary?.daily ?? [])].sort((a, b) => b.day - a.day));
  const retentionShort = $derived(summary && aggregateDays !== null
    && aggregateDays < summary.today - summary.start_day + summary.days + 1);
  const average = $derived(new Intl.NumberFormat(locale.language, { maximumFractionDigits: 1 }));

  const topLinks = $derived(summary?.top_links.map((link) => ({ label: link.title || `${link.host}/${link.slug}`, visits: link.total, href: href('links', link.id) })) ?? []);
  const domains = $derived(summary?.top_domains.map((domain) => ({
    label: domain.host, visits: domain.total, mono: true,
    href: host ? undefined : href('analytics', '', domainParams(domain.host)),
  })) ?? []);
  const tags = $derived(summary?.top_tags.map((tag) => ({ label: tag.tag, visits: tag.total })) ?? []);
  const classes = $derived(totals
    ? donutItems([
      { label: tr('insClassHuman'), total: totals.human },
      { label: tr('insClassBot'), total: totals.bot },
      { label: tr('insClassUnknown'), total: totals.unknown },
    ])
    : []);

  function breakdown(dimension: Dimension) {
    return summary?.breakdowns[dimension].map((row) => ({ label: categoryLabel(row.category), visits: row.total })) ?? [];
  }

  function showUnusual() {
    const section = document.getElementById('unusual');
    section?.scrollIntoView({ block: 'start' });
    section?.focus({ preventScroll: true });
  }

  function domainParams(next: string) {
    const values: Record<string, string> = { host: next };
    if ('start' in period) Object.assign(values, { start: period.start, end: period.end });
    else if (period.days !== 30) values.days = String(period.days);
    return values;
  }
</script>

<div class="page">
  <PageHeader title={tr('navAnalytics')} description={tr('insAnalyticsDescription')}>
    {#snippet actions()}
      <select class="select host-select" aria-label={tr('domainFilter')} value={host ?? ''} onchange={(event) => chooseHost(event.currentTarget.value || null)}>
        <option value="">{tr('insAllDomains')}</option>
        {#each hosts as option (option)}<option value={option}>{option}</option>{/each}
      </select>
      <div class="seg" role="group" aria-label={tr('analyticsRange')}>
        {#each presets as preset (preset)}
          <button type="button" aria-pressed={range === preset} onclick={() => choosePreset(preset)}>{dayCount(preset)}</button>
        {/each}
        <button type="button" aria-pressed={range === 'custom'} onclick={chooseCustom}>{tr('insCustom')}</button>
      </div>
      <Menu label={tr('insExport')} icon="download" variant="secondary">
        {#snippet children(close)}
          <button class="menu-item" type="button" role="menuitem" disabled={exporting || !summary} onclick={() => { close(); void exportReport('json'); }}><Icon name="file-down" />{tr('insExportJson')}</button>
          <button class="menu-item" type="button" role="menuitem" disabled={exporting || !summary} onclick={() => { close(); void exportReport('csv'); }}><Icon name="file-down" />{tr('insExportCsv')}</button>
        {/snippet}
      </Menu>
    {/snippet}
  </PageHeader>

  {#if range === 'custom'}
    <form class="panel custom" onsubmit={applyDates} novalidate aria-label={tr('insCustom')}>
      <div class="field">
        <label class="field-label" for="analytics-start">{tr('analyticsStartDate')}</label>
        <input bind:this={startInput} id="analytics-start" class="input" type="date" bind:value={startDate} min="1970-01-01" required
          aria-invalid={invalidDates ? 'true' : undefined} aria-describedby={invalidDates ? 'analytics-dates-error' : undefined} />
      </div>
      <div class="field">
        <label class="field-label" for="analytics-end">{tr('analyticsEndDate')}</label>
        <input id="analytics-end" class="input" type="date" bind:value={endDate} min="1970-01-01" required
          aria-invalid={invalidDates ? 'true' : undefined} aria-describedby={invalidDates ? 'analytics-dates-error' : undefined} />
      </div>
      <button class="btn" type="submit" disabled={loading}>{tr('analyticsApplyDates')}</button>
      {#if invalidDates}<p class="field-error" id="analytics-dates-error"><Icon name="circle-alert" />{tr('analyticsInvalidDates')}</p>{/if}
    </form>
  {/if}

  {#if host}
    <div class="notice filter" data-tone="neutral">
      <Icon name="globe" />
      <div><p>{tr('insHostFilter')} <b class="mono">{host}</b></p></div>
      <button class="btn" type="button" data-size="sm" onclick={() => chooseHost(null)}>{tr('analyticsAllDomains')}</button>
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
          factor: first.baseline_average > 0 ? average.format(first.today / first.baseline_average) : '—',
          days: num(traffic.baseline_days),
        })}</p>
      </div>
      {#if traffic.items.length > 1}
        <button class="btn" type="button" data-size="sm" onclick={showUnusual}>{tr('insShowLinks')}</button>
      {:else}
        <a class="btn" data-size="sm" href={href('links', first.id, { tab: 'analytics' })}>{tr('insOpenLink')}</a>
      {/if}
    </div>
  {/if}

  {#if summaryError}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr(summaryError)}</p></div>
      <button class="btn" type="button" data-size="sm" onclick={loadSummary}>{tr('retry')}</button>
    </div>
  {/if}

  <section class="panel kpis" aria-label={tr('insKeyFigures')} aria-busy={loading}>
    <div class="kpi">
      <span>{tr('insVisits')}</span><b class="num">{totals ? num(totals.redirect) : '—'}</b>
      <small>
        {#if summary && delta}
          {#if summary.previous_redirects > 0}
            <span class="trend" data-dir={delta.dir}><Icon name={delta.dir === 'down' ? 'trending-down' : 'trending-up'} />{percent(delta.value)}</span>{tr('insVsPreviousPeriod')}
          {:else}{tr('analyticsNoPriorRedirects')}{/if}
        {/if}
      </small>
    </div>
    <div class="kpi"><span>{tr('insHuman')}</span><b class="num">{totals ? num(totals.human) : '—'}</b><small>{totals ? tr('insShareOfVisits', { share: share(totals.human, totals.redirect) }) : ''}</small></div>
    <div class="kpi"><span>{tr('insBots')}</span><b class="num">{totals ? num(totals.bot) : '—'}</b><small>{tr('insBotsFoot')}</small></div>
    <div class="kpi"><span>{tr('insUnmatchedShort')}</span><b class="num">{totals ? num(totals.missing) : '—'}</b><small>{tr('insUnmatchedHint')}</small></div>
    <div class="kpi"><span>{tr('insBlocked')}</span><b class="num">{totals ? num(totals.blocked) : '—'}</b><small>{tr('insBlockedHint')}</small></div>
  </section>

  <section class="panel">
    <header class="panel-head">
      <div class="panel-title">
        <h2>{tr('insVisitsPerDay')}</h2>
        <p>{summary ? tr('insComparedWith', { range: periodLength, dates: dateRange }) : tr('analyticsUtc')}</p>
      </div>
      <span class="spacer"></span>
      <div class="legend">
        <span><i class="line" style:--c="var(--chart-1)"></i>{tr('insVisits')}</span>
        {#if previous}<span><i class="dashed"></i>{tr('insPreviousPeriod')}</span>{/if}
      </div>
    </header>
    <div class="panel-body chart-body">
      {#if loading}
        <div class="skeleton chart-skeleton" aria-hidden="true"></div>
        <span class="visually-hidden" role="status">{tr('loading')}</span>
      {:else if summary && (summary.totals.redirect > 0 || (previous ?? []).some(Boolean))}
        <TimeChart
          labels={summary.daily.map((point) => isoDay(point.day))}
          height={280}
          caption={tr('insVisitsPerDay')}
          series={[
            { name: tr('insVisits'), values: visits, color: 'var(--chart-1)', area: true },
            ...(previous ? [{ name: tr('insPreviousPeriod'), values: previous, color: 'var(--chart-previous)', dashed: true }] : []),
          ]}
        />
      {:else if summary}
        <p class="muted empty-chart">{tr('analyticsNoRedirects')}</p>
      {/if}
      {#if summary && dailyDesc.length}
        <details class="disclosure daily">
          <summary><Icon name="list-filter" />{tr('overviewDailyCounts')}<span class="meta">{tr('analyticsUtc')}</span><Icon name="chevron-right" /></summary>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (The daily table scrolls independently.) -->
          <div class="table-wrap daily-scroll" role="region" aria-label={tr('analyticsDaily')} tabindex="0">
            <table class="table" data-density="compact">
              <thead><tr><th scope="col">{tr('analyticsDayUtc')}</th><th scope="col" class="r">{tr('analyticsRedirects')}</th></tr></thead>
              <tbody>
                {#each dailyDesc as point (point.day)}
                  <tr>
                    <td><time datetime={isoDay(point.day)}>{new Intl.DateTimeFormat(locale.language, { dateStyle: 'medium', timeZone: 'UTC' }).format(new Date(point.day * DAY_MS))}</time></td>
                    <td class="r num">{num(point.redirect)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}
    </div>
    {#if retentionShort && aggregateDays !== null}
      <footer class="panel-foot"><p class="muted">{tr('analyticsRetentionNotice')} {dayCount(aggregateDays)}.</p></footer>
    {/if}
  </section>

  {#if summary}
    <div class="grid-3">
      <section class="panel span-2">
        <header class="panel-head">
          <div class="panel-title"><h2>{tr('analyticsTopLinks')}</h2></div>
          <span class="spacer"></span>
          <a class="btn" data-variant="ghost" data-size="sm" href={href('links')}>{tr('insAllLinks')}</a>
        </header>
        <div class="panel-body tight">{@render bars(topLinks, 8)}</div>
      </section>
      {#each dimensions as [dimension, label] (dimension)}
        {@const rows = breakdown(dimension)}
        <section class="panel">
          <header class="panel-head"><div class="panel-title"><h2>{tr(label)}</h2></div></header>
          {#if dimension === 'traffic_source' || dimension === 'device'}
            {@const items = donutItems(rows.map((row) => ({ label: row.label, total: row.visits })))}
            <div class="panel-body">
              {#if items.length}<Donut {items} center={donutCenter(items)} />{:else}<p class="muted">{tr('analyticsNoRedirects')}</p>{/if}
            </div>
          {:else}
            <div class="panel-body tight">{@render bars(rows, 7)}</div>
          {/if}
        </section>
        {#if dimension === 'traffic_source'}
          <section class="panel">
            <header class="panel-head"><div class="panel-title"><h2>{tr('domains')}</h2>{#if !host && domains.length > 1}<p>{tr('insDomainsHint')}</p>{/if}</div></header>
            <div class="panel-body tight">{@render bars(domains, 7)}</div>
          </section>
          <section class="panel">
            <header class="panel-head"><div class="panel-title"><h2>{tr('tags')}</h2><p>{tr('insTagsHint')}</p></div></header>
            <div class="panel-body tight">{@render bars(tags, 6, tr('analyticsNoTaggedRedirects'))}</div>
            {#if tags.length}<footer class="panel-foot"><p class="muted">{tr('analyticsTagAssignments')}</p></footer>{/if}
          </section>
        {/if}
      {/each}
      <section class="panel">
        <header class="panel-head"><div class="panel-title"><h2>{tr('insVisitClass')}</h2><p>{tr('insVisitClassHint')}</p></div></header>
        <div class="panel-body">
          {#if classes.length}<Donut items={classes} center={donutCenter(classes)} />{:else}<p class="muted">{tr('analyticsNoRedirects')}</p>{/if}
        </div>
      </section>

      <section class="panel span-2" id="unusual" tabindex="-1" aria-labelledby="unusual-title">
        <header class="panel-head">
          <div class="panel-title">
            <h2 id="unusual-title">{tr('trafficUnusual')}</h2>
            {#if traffic}<p>{tr('trafficUnusualHint', { minimum: num(traffic.minimum_redirects), multiplier: num(traffic.multiplier), days: num(traffic.baseline_days) })}</p>{/if}
          </div>
        </header>
        {#if trafficError}
          <div class="panel-body">
            <div class="notice" data-tone="error" role="alert">
              <Icon name="circle-alert" /><div><p>{tr(trafficError)}</p></div>
              <button class="btn" type="button" data-size="sm" onclick={loadTraffic}>{tr('retry')}</button>
            </div>
          </div>
        {:else if traffic?.items.length}
          <div class="table-wrap">
            <table class="table" data-density="compact">
              <thead><tr><th scope="col">{tr('shortLink')}</th><th scope="col" class="r">{tr('trafficToday')}</th><th scope="col" class="r">{tr('trafficAverage')}</th></tr></thead>
              <tbody>
                {#each traffic.items as link (link.id)}
                  <tr>
                    <td><a class="cell-link" href={href('links', link.id, { tab: 'analytics' })}>{#if link.title}<span class="truncate">{link.title}</span>{/if}<span class="mono muted">{link.host}/{link.slug}</span></a></td>
                    <td class="r num">{num(link.today)}</td>
                    <td class="r num">{average.format(link.baseline_average)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if traffic.has_more}<footer class="panel-foot"><p class="muted">{tr('trafficMore')}</p></footer>{/if}
        {:else if traffic}
          <div class="panel-body"><p class="muted">{traffic.analytics_enabled ? tr('trafficNone') : tr('trafficAnalyticsOff')}</p></div>
        {:else}
          <div class="panel-body"><p class="muted">{tr('loading')}</p></div>
        {/if}
      </section>

      <section class="panel">
        <header class="panel-head"><div class="panel-title"><h2>{tr('insMoreFigures')}</h2><p>{dateRange}</p></div></header>
        <div class="panel-body">
          <dl class="facts">
            <dt>{tr('analyticsRootFallback')}</dt><dd class="num">{num(summary.totals.root_fallback)}</dd>
            <dt>{tr('analyticsRootMissing')}</dt><dd class="num">{num(summary.totals.root_missing)}</dd>
            <dt>{tr('analyticsPreviousRedirects')}</dt><dd class="num">{num(summary.previous_redirects)}</dd>
            <dt>{tr('analyticsRedirectChange')}</dt>
            <dd class="num">{summary.previous_redirects > 0 && delta ? percent(delta.value) : tr('analyticsNoPriorRedirects')}</dd>
            {#if summary.dropped !== null}<dt>{tr('analyticsDropped')}</dt><dd class="num">{num(summary.dropped)}</dd>{/if}
          </dl>
        </div>
      </section>
    </div>
    <p class="muted footnote">{tr('analyticsClassificationHint')}</p>
  {/if}
</div>

{#snippet bars(items: { label: string; visits: number; mono?: boolean; href?: string }[], limit: number, empty = tr('analyticsNoRedirects'))}
  {#if items.length}<BarList {items} {limit} />{:else}<p class="muted">{empty}</p>{/if}
{/snippet}

<style>
  .host-select {
    width: auto;
    max-width: 240px;
  }
  .custom {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3) var(--space-4);
    padding: var(--space-4) var(--space-5);
  }
  .custom .field-error {
    flex-basis: 100%;
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
  }
  .kpi {
    display: grid;
    gap: 4px;
    align-content: start;
    padding: var(--space-4) var(--space-5);
    min-width: 0;
  }
  .kpi + .kpi {
    border-left: 1px solid var(--border-subtle);
  }
  .kpi > span {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .kpi b {
    font-size: var(--text-2xl);
    line-height: var(--leading-2xl);
    font-weight: var(--weight-semibold);
    letter-spacing: var(--tracking-display);
  }
  .kpi small {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 6px;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .chart-body {
    display: grid;
    gap: var(--space-4);
    padding-left: var(--space-3);
  }
  .chart-skeleton {
    height: 280px;
  }
  .empty-chart {
    padding: var(--space-6) 0;
    text-align: center;
  }
  .daily {
    margin-left: var(--space-3);
  }
  .daily-scroll {
    max-height: 330px;
    overflow: auto;
  }
  .grid-3 {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-6);
    align-items: stretch;
  }
  .span-2 {
    grid-column: span 2;
  }
  .tight {
    padding: var(--space-3);
  }
  .tight > p {
    padding: var(--space-2);
  }
  .cell-link {
    display: grid;
    gap: 1px;
    min-width: 0;
    color: var(--text);
    font-weight: var(--weight-medium);
    text-decoration: none;
  }
  .cell-link:hover > .truncate {
    color: var(--accent);
  }
  .cell-link .mono {
    font-size: var(--text-xs);
    font-weight: var(--weight-regular);
  }
  .footnote {
    font-size: var(--text-xs);
  }
  @media (max-width: 1200px) {
    .kpis {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
    .kpi:nth-child(4) {
      border-left: 0;
    }
    .kpi:nth-child(n + 4) {
      border-top: 1px solid var(--border-subtle);
    }
    .grid-3 {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  @media (max-width: 760px) {
    .kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .kpi,
    .kpi + .kpi {
      border-left: 0;
      border-top: 1px solid var(--border-subtle);
    }
    .kpi:nth-child(-n + 2) {
      border-top: 0;
    }
    .kpi:nth-child(even) {
      border-left: 1px solid var(--border-subtle);
    }
    .grid-3 {
      grid-template-columns: minmax(0, 1fr);
    }
    .span-2 {
      grid-column: auto;
    }
    .host-select {
      max-width: none;
      flex: 1;
    }
  }
</style>
