<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api, TrafficReport } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';

  let { t, api, language, host = null, quiet = false }: {
    t: Messages; api: Api; language: Language; host?: string | null; quiet?: boolean;
  } = $props();

  const headingId = $props.id();
  const whole = $derived(new Intl.NumberFormat(language));
  const average = $derived(new Intl.NumberFormat(language, { maximumFractionDigits: 1 }));
  let report = $state.raw<TrafficReport>();
  let error = $state<keyof Messages | null>(null);
  let request: AbortController | undefined;

  onMount(() => () => request?.abort());

  $effect(() => {
    void load(host);
  });

  async function load(selected: string | null) {
    request?.abort();
    const current = new AbortController();
    request = current;
    error = null;
    try {
      const query = selected ? `?host=${encodeURIComponent(selected)}` : '';
      const result = await api<TrafficReport>(`/analytics/traffic${query}`, 'GET', undefined, 'json', current.signal);
      if (!current.signal.aborted) report = result;
    } catch (failure) {
      if (!current.signal.aborted) error = problemMessage(failure);
    }
  }

  const hint = $derived(report
    ? t.trafficUnusualHint
      .replace('{minimum}', whole.format(report.minimum_redirects))
      .replace('{multiplier}', whole.format(report.multiplier))
      .replace('{days}', whole.format(report.baseline_days))
    : '');
</script>

{#if error || (report && (!quiet || report.items.length))}
  <section class="panel" aria-labelledby={headingId}>
    <div class="toolbar">
      <div class="stack compact">
        <h2 id={headingId}>{t.trafficUnusual}</h2>
        {#if report}<p class="muted">{hint}</p>{/if}
      </div>
    </div>
    {#if error}
      <div class="panel-foot between wrap">
        <p class="notice error" role="alert">{t[error]}</p>
        <button class="button" type="button" onclick={() => load(host)}>{t.retry}</button>
      </div>
    {:else if report && report.items.length}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">{t.shortLink}</th>
              <th scope="col" class="align-right">{t.trafficToday}</th>
              <th scope="col" class="align-right">{t.trafficAverage}</th>
            </tr>
          </thead>
          <tbody>
            {#each report.items as link (link.id)}
              <tr>
                <td>
                  <div class="link-name">
                    {#if link.title}<strong>{link.title}</strong>{/if}
                    <span class="mono muted">{link.host}/{link.slug}</span>
                  </div>
                </td>
                <td class="align-right table-count">{whole.format(link.today)}</td>
                <td class="align-right table-count">{average.format(link.baseline_average)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if report.has_more}<div class="panel-foot"><p class="muted">{t.trafficMore}</p></div>{/if}
    {:else if report}
      <div class="panel-foot"><p>{report.analytics_enabled ? t.trafficNone : t.trafficAnalyticsOff}</p></div>
    {/if}
  </section>
{/if}

<style>
  .wrap { flex-wrap: wrap; }
  .link-name { display: grid; min-width: 0; overflow-wrap: anywhere; }
</style>
