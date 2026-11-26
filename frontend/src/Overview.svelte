<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api } from './api';
  import { formatDate, problemMessage } from './format';
  import type { Language, Messages } from './i18n';
  import Icon from './Icon.svelte';

  type Destination = 'links' | 'analytics' | 'monitoring';
  const statusLabels = {
    active: 'enabled', scheduled: 'overviewScheduled', expired: 'overviewExpired',
    disabled: 'disabled', archived: 'overviewArchived', exhausted: 'overviewExhausted',
  } as const satisfies Record<string, keyof Messages>;
  type LinkStatus = keyof typeof statusLabels;
  type LinkEntry = { id: number; host: string; slug: string; title: string };

  interface Dashboard {
    links: Record<LinkStatus | 'total' | 'expiring_soon', number>;
    active_domains: number;
    recent_links: (LinkEntry & { created_at: string; state: LinkStatus })[];
    monitoring: { enabled: boolean; total: number; healthy: number; attention: number; pending: number; last_checked: number | null };
  }

  interface Traffic {
    start_day: number;
    end_day: number;
    totals: { redirect: number };
    previous_redirects: number;
    daily: { day: number; redirect: number }[];
    top_links: (LinkEntry & { total: number })[];
  }

  let { t, language, api, oncreate, onnavigate }: {
    t: Messages; language: Language; api: Api; oncreate: () => void; onnavigate: (page: Destination) => void;
  } = $props();

  const pageId = $props.id();
  const dayMillis = 86_400_000;
  const periods = [7, 30, 90] as const;
  const monitorLabels = [
    ['healthy', 'monitorHealthy'], ['attention', 'overviewAttention'], ['pending', 'overviewPending'],
  ] as const;

  let days = $state(30);
  let dashboard = $state.raw<Dashboard>();
  let dashboardError = $state<keyof Messages | null>(null);
  let traffic = $state.raw<Traffic>();
  let trafficError = $state<keyof Messages | null>(null);
  const dashboardRequest = new AbortController();
  let trafficRequest: AbortController | undefined;

  const whole = $derived(new Intl.NumberFormat(language));
  const dayCount = $derived(new Intl.NumberFormat(language, { style: 'unit', unit: 'day', unitDisplay: 'long' }));
  const date = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeZone: 'UTC' }));
  const peak = $derived(Math.max(1, ...(traffic?.daily.map((point) => point.redirect) ?? [])));

  onMount(() => {
    void loadDashboard();
    void loadTraffic();
    return () => {
      dashboardRequest.abort();
      trafficRequest?.abort();
    };
  });

  async function loadDashboard() {
    dashboardError = null;
    try {
      dashboard = await api<Dashboard>('/overview', 'GET', undefined, 'json', dashboardRequest.signal);
    } catch (error) {
      if (!dashboardRequest.signal.aborted) dashboardError = problemMessage(error);
    }
  }

  async function loadTraffic() {
    trafficRequest?.abort();
    const request = new AbortController();
    trafficRequest = request;
    traffic = undefined;
    trafficError = null;
    try {
      const report = await api<Traffic>(`/analytics/summary?days=${days}`, 'GET', undefined, 'json', request.signal);
      if (!request.signal.aborted) traffic = report;
    } catch (error) {
      if (!request.signal.aborted) trafficError = problemMessage(error);
    }
  }

  function changePeriod(event: Event & { currentTarget: HTMLSelectElement }) {
    days = Number(event.currentTarget.value);
    void loadTraffic();
  }

  function utcDay(day: number) {
    return new Date(day * dayMillis);
  }
</script>

<div class="page-head">
  <div class="stack compact">
    <h1>{t.overview}</h1>
    {#if traffic}<p class="muted">{date.formatRange(utcDay(traffic.start_day), utcDay(traffic.end_day))} ({t.analyticsUtc})</p>{/if}
  </div>
  <div class="row actions">
    <label class="field">
      <span>{t.analyticsRange}</span>
      <select value={days} onchange={changePeriod}>
        {#each periods as period (period)}<option value={period}>{dayCount.format(period)}</option>{/each}
      </select>
    </label>
    <button class="button primary" type="button" onclick={oncreate}><Icon name="plus" />{t.addLink}</button>
  </div>
</div>

<div class="stack">
  {#if dashboardError}
    <div class="panel panel-pad between wrap">
      <p class="notice error" role="alert">{t[dashboardError]}</p>
      <button class="button" type="button" onclick={loadDashboard}>{t.retry}</button>
    </div>
  {:else if !dashboard}
    <p role="status">{t.loading}</p>
  {/if}

  {#if dashboard || traffic}
    <dl class="metrics">
      {#if dashboard}
        <div class="panel stat">
          <dt><Icon name="link" />{t.overviewTotalLinks}</dt>
          <dd>{whole.format(dashboard.links.total)}<small>{t.overviewActiveLinks} · {whole.format(dashboard.links.active)}</small></dd>
        </div>
      {/if}
      {#if traffic}
        <div class="panel stat">
          <dt><Icon name="analytics" />{t.analyticsRedirects}</dt>
          <dd>{whole.format(traffic.totals.redirect)}<small>{t.analyticsPreviousRedirects} · {whole.format(traffic.previous_redirects)}</small></dd>
        </div>
      {/if}
      {#if dashboard}
        <div class="panel stat">
          <dt><Icon name="globe" />{t.overviewActiveDomains}</dt>
          <dd>{whole.format(dashboard.active_domains)}</dd>
        </div>
        <div class="panel stat">
          <dt><Icon name="history" />{t.overviewExpiring}</dt>
          <dd>{whole.format(dashboard.links.expiring_soon)}<small>{t.overviewNextSevenDays}</small></dd>
        </div>
      {/if}
    </dl>
  {/if}

  <div class="board">
    <section class="panel stack panel-pad" aria-labelledby={pageId + '-traffic'}>
      <div class="between wrap">
        <h2 id={pageId + '-traffic'}>{t.analyticsDaily}</h2>
        <button class="button" type="button" onclick={() => onnavigate('analytics')}>{t.overviewViewAnalytics}</button>
      </div>
      {#if trafficError}
        <div class="between wrap">
          <p class="notice error" role="alert">{t[trafficError]}</p>
          <button class="button" type="button" onclick={loadTraffic}>{t.retry}</button>
        </div>
      {:else if !traffic}
        <p role="status">{t.loading}</p>
      {:else if traffic.totals.redirect === 0}
        <p>{t.analyticsNoRedirects}</p>
      {:else}
        <div aria-hidden="true">
          <div class="plot">
            <div class="scale"><span>{whole.format(peak)}</span><span>{whole.format(0)}</span></div>
            <div class="bars">
              {#each traffic.daily as point (point.day)}
                <div class="bar-slot" title={`${date.format(utcDay(point.day))} · ${whole.format(point.redirect)}`}>
                  <span class="bar" style:height={`${100 * point.redirect / peak}%`}></span>
                </div>
              {/each}
            </div>
          </div>
          <div class="between chart-dates muted">
            <span>{date.format(utcDay(traffic.start_day))}</span>
            <span>{date.format(utcDay(traffic.end_day))}</span>
          </div>
        </div>
        <details>
          <summary>{t.overviewDailyCounts}</summary>
          <div class="table-wrap daily-counts">
            <table>
              <thead><tr><th scope="col">{t.analyticsDayUtc}</th><th class="align-right" scope="col">{t.analyticsRedirects}</th></tr></thead>
              <tbody>
                {#each traffic.daily as point (point.day)}
                  <tr><td>{date.format(utcDay(point.day))}</td><td class="align-right table-count">{whole.format(point.redirect)}</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}
    </section>

    {#if dashboard}
      <section class="panel stack panel-pad" aria-labelledby={pageId + '-health'}>
        <h2 id={pageId + '-health'}>{t.overviewLinkHealth}</h2>
        {#if !dashboard.monitoring.enabled}
          <p>{t.overviewMonitorOff}</p>
        {:else if dashboard.monitoring.total === 0}
          <p>{t.monitorEmpty}</p>
        {:else}
          <div class="health-bar" aria-hidden="true">
            {#each monitorLabels as [key] (key)}
              <span class={key} style:width={`${dashboard.monitoring[key] / dashboard.monitoring.total * 100}%`}></span>
            {/each}
          </div>
          <dl class="health-list stack compact">
            {#each monitorLabels as [key, label] (key)}
              <div class="between"><dt>{t[label]}</dt><dd>{whole.format(dashboard.monitoring[key])}</dd></div>
            {/each}
          </dl>
          {#if dashboard.monitoring.last_checked !== null}
            <p class="muted">
              {t.overviewLastCheck} · {formatDate(new Date(dashboard.monitoring.last_checked * 1000).toISOString(), language, true)}
            </p>
          {/if}
        {/if}
        <div><button class="button" type="button" onclick={() => onnavigate('monitoring')}>{t.overviewViewMonitor}</button></div>
      </section>
    {/if}
  </div>

  {#if dashboard || traffic}
    <div class="board">
      {#if dashboard}
        <section class="panel" aria-labelledby={pageId + '-recent'}>
          <div class="toolbar between wrap">
            <h2 id={pageId + '-recent'}>{t.overviewRecentLinks}</h2>
            <button class="button" type="button" onclick={() => onnavigate('links')}>{t.overviewViewLinks}</button>
          </div>
          {#if dashboard.recent_links.length}
            <div class="table-wrap">
              <table>
                <thead><tr><th scope="col">{t.shortLink}</th><th scope="col">{t.status}</th><th scope="col">{t.created}</th></tr></thead>
                <tbody>
                  {#each dashboard.recent_links as link (link.id)}
                    <tr>
                      <td>{@render linkName(link)}</td>
                      <td><span class:active={link.state === 'active'} class="link-status">{t[statusLabels[link.state]]}</span></td>
                      <td><time datetime={link.created_at}>{formatDate(link.created_at, language, false, 'UTC')}</time></td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {:else}
            <div class="panel-foot"><p>{t.noLinks}</p></div>
          {/if}
        </section>
      {/if}
      {#if traffic?.top_links.length}
        <section class="panel" aria-labelledby={pageId + '-top'}>
          <div class="toolbar"><h2 id={pageId + '-top'}>{t.analyticsTopLinks}</h2></div>
          <div class="table-wrap">
            <table>
              <thead><tr><th scope="col">{t.shortLink}</th><th scope="col" class="align-right">{t.analyticsRedirects}</th></tr></thead>
              <tbody>
                {#each traffic.top_links.slice(0, 5) as link (link.id)}
                  <tr><td>{@render linkName(link)}</td><td class="align-right table-count">{whole.format(link.total)}</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}
    </div>
  {/if}
</div>

{#snippet linkName(link: LinkEntry)}
  <div class="link-name">
    {#if link.title}<strong>{link.title}</strong>{/if}
    <span class="mono muted">{link.host}/{link.slug}</span>
  </div>
{/snippet}

<style>
  .actions { align-items: end; }
  .actions, .wrap { flex-wrap: wrap; }
  .metrics { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)); gap: 16px; margin: 0; }
  .stat { padding: 16px; }
  .stat dt { display: flex; align-items: center; gap: 8px; color: var(--sub); }
  dd { margin: 0; font-variant-numeric: tabular-nums; font-weight: 650; }
  .stat dd { margin-block-start: 4px; font-size: clamp(24px, 4vw, 34px); line-height: 1.2; }
  .stat small { display: block; font-size: 12px; font-weight: 450; line-height: 1.45; margin-top: 5px; }
  .board { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); align-items: start; gap: 16px; }
  .board > :only-child { grid-column: 1 / -1; }
  .plot { display: flex; align-items: stretch; gap: 8px; block-size: 150px; }
  .scale { display: flex; flex-direction: column; justify-content: space-between; text-align: end; color: var(--muted); font-variant-numeric: tabular-nums; }
  .bars { display: flex; gap: 2px; flex: 1; min-width: 0; border-bottom: 1px solid var(--line); border-top: 1px solid var(--line); }
  .bar-slot { flex: 1; display: flex; flex-direction: column; justify-content: end; min-width: 0; }
  .bar { min-height: 1px; border-radius: 2px 2px 0 0; background: var(--accent-text); }
  .chart-dates { gap: 4px; margin-top: 4px; font-size: 12px; }
  details > summary { cursor: pointer; color: var(--sub); }
  .daily-counts { max-block-size: 250px; margin-top: 8px; }
  .health-bar { display: flex; height: 8px; border-radius: 8px; overflow: hidden; background: var(--line); }
  .health-bar .healthy { background: var(--positive); }
  .health-bar .attention { background: var(--danger); }
  .health-bar .pending { background: var(--control); }
  .health-list { margin: 0; }
  .health-list dt { color: var(--sub); }
  .link-name { display: grid; min-width: 0; overflow-wrap: anywhere; }
  .link-status { white-space: nowrap; }
  .link-status.active { color: var(--positive); }

  @media (max-width: 960px) { .board { grid-template-columns: 1fr; } }
</style>
