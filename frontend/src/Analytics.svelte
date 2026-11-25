<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from './api';
  import type { Api } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  interface Summary {
    days: number;
    today: number;
    totals: Record<'redirect' | 'blocked' | 'missing' | 'human' | 'bot' | 'unknown', number>;
    previous_redirects: number;
    daily: { day: number; redirect: number }[];
    top_links: { id: number; slug: string; host: string; title: string; total: number }[];
    dropped: number;
  }

  interface Settings {
    enabled: boolean;
    referrers: boolean;
    classify: boolean;
    honor_dnt: boolean;
    detail_days: number;
    aggregate_days: number;
  }

  type SettingsDraft = Omit<Settings, 'detail_days' | 'aggregate_days'> & {
    detail_days: number | undefined;
    aggregate_days: number | undefined;
  };

  let { t, language, api }: { t: Messages; language: Language; api: Api } = $props();

  const ranges = [7, 30, 90, 365];
  const metrics = [
    ['redirect', 'analyticsRedirects'], ['blocked', 'analyticsBlocked'], ['missing', 'analyticsMissing'],
    ['human', 'analyticsHuman'], ['bot', 'analyticsBot'], ['unknown', 'analyticsUnknown'],
  ] as const;
  const collectionFields = [
    ['enabled', 'analyticsEnabled'], ['referrers', 'analyticsReferrers'],
    ['classify', 'analyticsClassify'], ['honor_dnt', 'analyticsHonorDnt'],
  ] as const;
  const panelId = $props.id();

  let days = $state(30);
  let summary = $state.raw<Summary | null>(null);
  let summaryLoading = $state(true);
  let summaryError = $state<keyof Messages | null>(null);
  let summaryRequest = 0;

  let settings = $state.raw<Settings | null>(null);
  let draft = $state<SettingsDraft | null>(null);
  let settingsLoading = $state(true);
  let settingsError = $state<keyof Messages | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let saving = $state(false);

  let confirmClear = $state(false);
  let clearing = $state(false);
  let clearError = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);

  const whole = $derived(new Intl.NumberFormat(language));
  const dayCount = $derived(new Intl.NumberFormat(language, { style: 'unit', unit: 'day', unitDisplay: 'long' }));
  const percent = $derived(new Intl.NumberFormat(language, {
    style: 'percent', signDisplay: 'exceptZero', maximumFractionDigits: 1,
  }));
  const utc = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeZone: 'UTC' }));
  const daily = $derived([...(summary?.daily ?? [])].sort((a, b) => b.day - a.day));

  onMount(() => {
    void loadSummary();
    void loadSettings();
  });

  async function loadSummary() {
    const request = ++summaryRequest;
    summaryLoading = true;
    summaryError = null;
    summary = null;
    try {
      const result = await api<Summary>(`/analytics/summary?days=${days}`);
      if (request === summaryRequest) summary = result;
    } catch (error) {
      if (request === summaryRequest) summaryError = problemMessage(error);
    } finally {
      if (request === summaryRequest) summaryLoading = false;
    }
  }

  function changeRange(event: Event & { currentTarget: HTMLSelectElement }) {
    days = Number(event.currentTarget.value);
    void loadSummary();
  }

  async function loadSettings() {
    settingsLoading = true;
    settingsError = null;
    try {
      settings = await api<Settings>('/analytics/settings');
      draft = { ...settings };
    } catch (error) {
      settingsError = problemMessage(error);
    } finally {
      settingsLoading = false;
    }
  }

  async function saveSettings(event: SubmitEvent) {
    event.preventDefault();
    if (!draft || saving || clearing) return;

    formError = null;
    notice = null;
    const detailDays = draft.detail_days ?? 0;
    const aggregateDays = draft.aggregate_days ?? 0;
    const validDays = [detailDays, aggregateDays]
      .every((days) => Number.isInteger(days) && days >= 1 && days <= 3650);
    if (!validDays || aggregateDays < detailDays) {
      formError = 'invalidAnalyticsRetention';
      return;
    }

    saving = true;
    try {
      const values = { ...draft, detail_days: detailDays, aggregate_days: aggregateDays };
      settings = await api<Settings>('/analytics/settings', 'PATCH', values);
      draft = { ...settings };
      notice = 'analyticsSettingsSaved';
      void loadSummary();
    } catch (error) {
      formError = error instanceof ApiError && error.code === 'invalid_retention'
        ? 'invalidAnalyticsRetention'
        : problemMessage(error);
    } finally {
      saving = false;
    }
  }

  async function clearData(event: SubmitEvent) {
    event.preventDefault();
    if (!confirmClear || clearing || saving) return;

    clearing = true;
    clearError = null;
    notice = null;
    try {
      await api<void>('/analytics/data', 'DELETE');
      confirmClear = false;
      notice = 'analyticsDataCleared';
      void loadSummary();
    } catch (error) {
      clearError = problemMessage(error);
    } finally {
      clearing = false;
    }
  }

  function utcDay(day: number): Date {
    return new Date(day * 86_400_000);
  }
</script>

<div class="page-head">
  <h1>{t.analytics}</h1>
  <label class="field">
    <span>{t.analyticsRange}</span>
    <select value={days} onchange={changeRange}>
      {#each ranges as range (range)}<option value={range}>{dayCount.format(range)}</option>{/each}
    </select>
  </label>
</div>

{#if notice}<p class="notice page-notice" role="status">{t[notice]}</p>{/if}

<div class="stack">
  {#if summaryLoading}
    <div class="panel panel-pad"><p role="status">{t.loading}</p></div>
  {:else if summaryError}
    <div class="panel panel-pad between status-line">
      <p class="notice error" role="alert">{t[summaryError]}</p>
      <button class="button" type="button" onclick={loadSummary}>{t.retry}</button>
    </div>
  {:else if summary}
    <section class="panel panel-pad stack" aria-labelledby={panelId + '-summary'}>
      <div class="between status-line">
        <h2 id={panelId + '-summary'}>{t.analyticsSummary}</h2>
        <p class="muted">
          <time datetime={utcDay(summary.today - summary.days + 1).toISOString().slice(0, 10)}>{utc.format(utcDay(summary.today - summary.days + 1))}</time>
          –
          <time datetime={utcDay(summary.today).toISOString().slice(0, 10)}>{utc.format(utcDay(summary.today))}</time>
          ({t.analyticsUtc})
        </p>
      </div>
      <dl class="metric-list">
        {#each metrics as [key, label] (key)}
          <div><dt>{t[label]}</dt><dd>{whole.format(summary.totals[key])}</dd></div>
        {/each}
      </dl>
      <dl class="report-notes">
        <div><dt>{t.analyticsPreviousRedirects}</dt><dd>{whole.format(summary.previous_redirects)}</dd></div>
        <div>
          <dt>{t.analyticsRedirectChange}</dt>
          <dd>
            {summary.previous_redirects > 0
              ? percent.format((summary.totals.redirect - summary.previous_redirects) / summary.previous_redirects)
              : t.analyticsNoPriorRedirects}
          </dd>
        </div>
        <div><dt>{t.analyticsDropped}</dt><dd>{whole.format(summary.dropped)}</dd></div>
      </dl>
      {#if settings && settings.aggregate_days < summary.days * 2}
        <p class="muted">{t.analyticsRetentionNotice} {dayCount.format(settings.aggregate_days)}.</p>
      {/if}
    </section>

    <div class="report-tables">
      <section class="panel" aria-labelledby={panelId + '-daily'}>
        <div class="toolbar"><h2 id={panelId + '-daily'}>{t.analyticsDaily}</h2></div>
        {#if daily.length}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (The daily table scrolls independently.) -->
          <div class="daily-scroll table-wrap" role="region" aria-labelledby={panelId + '-daily'} tabindex="0">
            <table>
              <thead><tr><th scope="col">{t.analyticsDayUtc}</th><th scope="col" class="align-right">{t.analyticsRedirects}</th></tr></thead>
              <tbody>
                {#each daily as point (point.day)}
                  {@const date = utcDay(point.day)}
                  <tr>
                    <td><time datetime={date.toISOString().slice(0, 10)}>{utc.format(date)}</time></td>
                    <td class="align-right table-count">{whole.format(point.redirect)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div class="panel-foot"><p>{t.analyticsNoRedirects}</p></div>
        {/if}
      </section>

      <section class="panel" aria-labelledby={panelId + '-top'}>
        <div class="toolbar"><h2 id={panelId + '-top'}>{t.analyticsTopLinks}</h2></div>
        {#if summary.top_links.length}
          <div class="table-wrap">
            <table class="top-links">
              <thead><tr><th scope="col">{t.shortLink}</th><th scope="col" class="align-right">{t.analyticsRedirects}</th></tr></thead>
              <tbody>
                {#each summary.top_links as link (link.id)}
                  <tr>
                    <td>
                      {#if link.title}<strong>{link.title}</strong><br />{/if}
                      <span class="mono">{link.host}/{link.slug}</span>
                    </td>
                    <td class="align-right table-count">{whole.format(link.total)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div class="panel-foot"><p>{t.analyticsNoRedirects}</p></div>
        {/if}
      </section>
    </div>
  {/if}

  <section class="panel panel-pad stack" aria-labelledby={panelId + '-settings'} aria-busy={settingsLoading || saving}>
    <h2 id={panelId + '-settings'}>{t.analyticsSettings}</h2>
    {#if settingsLoading}
      <p role="status">{t.loading}</p>
    {:else if settingsError}
      <div class="between status-line">
        <p class="notice error" role="alert">{t[settingsError]}</p>
        <button class="button" type="button" onclick={loadSettings}>{t.retry}</button>
      </div>
    {:else if draft}
      <form class="stack" onsubmit={saveSettings}>
        <fieldset class="permissions" disabled={saving || clearing}>
          <legend>{t.analyticsCollection}</legend>
          {#each collectionFields as [key, label] (key)}
            <label><input type="checkbox" name={key} bind:checked={draft[key]} />{t[label]}</label>
          {/each}
        </fieldset>
        <div class="retention-fields">
          <label class="field">
            <span>{t.analyticsDetailRetention}</span>
            <input type="number" name="detail_days" min="1" max="3650" step="1" required bind:value={draft.detail_days} disabled={saving || clearing} />
          </label>
          <label class="field">
            <span>{t.analyticsAggregateRetention}</span>
            <input type="number" name="aggregate_days" min="1" max="3650" step="1" required bind:value={draft.aggregate_days} disabled={saving || clearing} />
          </label>
        </div>
        {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
        <div class="row">
          <button class="button primary" type="submit" disabled={saving || clearing}>{saving ? t.saving : t.save}</button>
        </div>
      </form>
    {/if}
    <div class="data-actions">
      <button class="button danger" type="button" disabled={saving || clearing} onclick={() => { clearError = null; confirmClear = true; }}>
        <Icon name="trash" />{t.analyticsClearData}
      </button>
    </div>
  </section>
</div>

<Dialog {t} title={t.analyticsClearData} open={confirmClear} busy={clearing} onclose={() => confirmClear = false}>
  {#if confirmClear}
    <form onsubmit={clearData} aria-busy={clearing}>
      <div class="dialog-body stack compact">
        <p>{t.analyticsClearHint}</p>
        {#if clearError}<p class="notice error" role="alert">{t[clearError]}</p>{/if}
      </div>
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={clearing} onclick={() => confirmClear = false}>{t.cancel}</button>
        <button class="button danger" type="submit" disabled={clearing}>{clearing ? t.analyticsClearing : t.analyticsClearData}</button>
      </footer>
    </form>
  {/if}
</Dialog>

<style>
  .status-line { flex-wrap: wrap; }
  .status-line > * { min-width: 0; }

  .metric-list, .report-notes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 145px), 1fr));
    gap: 12px;
    margin: 0;
  }

  .metric-list > div {
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--rail);
  }

  dt { color: var(--sub); }
  dd { margin: 4px 0 0; font-variant-numeric: tabular-nums; }
  .metric-list dd { font-size: 24px; font-weight: 700; overflow-wrap: anywhere; }
  .report-notes { border-top: 1px solid var(--line); padding-top: 12px; }
  .report-notes dd { font-weight: 650; }
  .report-tables, .retention-fields { display: grid; gap: 16px; grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .report-tables { align-items: start; }
  .daily-scroll { max-block-size: 330px; overflow: auto; }
  .daily-scroll th { position: sticky; inset-block-start: 0; }
  .top-links th:last-child { width: 30%; }
  .data-actions { border-top: 1px solid var(--line); padding-top: 16px; }

  @media (max-width: 760px) {
    .report-tables { grid-template-columns: 1fr; }
  }

  @media (max-width: 460px) {
    .retention-fields { grid-template-columns: 1fr; }
  }
</style>
