<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';
  import { useTimeZone } from './timezone';

  type Status = 'pending' | 'delivered' | 'failed' | 'discarded';
  interface Delivery {
    id: number; event: string; occurred_at: number; status: Status; attempts: number;
    last_error: string | null; next_attempt_at: number; delivered_at: number | null;
  }
  interface Report {
    enabled: boolean; origin: string | null; connector: boolean; events: string[];
    click_milestones: number[]; expiry_notice_hours: number | null;
    counts: Record<Status, number>; items: Delivery[];
  }

  let { api, t, language }: { api: Api; t: Messages; language: Language } = $props();
  const headingId = $props.id();
  const readTimeZone = useTimeZone();
  const dateTime = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short', timeZone: readTimeZone() }));
  const whole = $derived(new Intl.NumberFormat(language));
  const hours = $derived(new Intl.NumberFormat(language, { style: 'unit', unit: 'hour', unitDisplay: 'long' }));
  let report = $state<Report>();
  let working = $state(false);
  let error = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);

  const statusLabels = $derived<Record<Status, string>>({
    pending: t.notificationPending, delivered: t.notificationDelivered,
    failed: t.notificationFailed, discarded: t.notificationDiscarded,
  });
  const errorLabels = $derived<Record<string, string>>({
    blocked: t.monitorBlocked, busy: t.monitorBusy, dns: t.monitorDns, timeout: t.monitorTimeout,
    connection: t.monitorConnection, protocol: t.notificationRejected, restored: t.notificationRestored,
    stale: t.notificationStale,
  });

  onMount(() => { void load(); });

  async function load() {
    error = null;
    try {
      report = await api<Report>('/notifications');
    } catch (issue) {
      error = problemMessage(issue);
    }
  }

  async function sendTest() {
    if (working) return;
    working = true;
    error = null;
    notice = null;
    try {
      await api('/notifications/test', 'POST');
      notice = 'notificationTestQueued';
      await load();
    } catch (issue) {
      error = problemMessage(issue);
    } finally {
      working = false;
    }
  }

  function stamp(value: number) {
    return dateTime.format(new Date(value * 1000));
  }
</script>

<section class="panel panel-pad stack" aria-labelledby={headingId} aria-busy={working}>
  <div class="between wrap">
    <h2 id={headingId}>{t.notifications}</h2>
    <button class="button" type="button" disabled={working} onclick={load}>{t.monitorRefresh}</button>
  </div>
  {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
  {#if notice}<p class="notice" role="status">{t[notice]}</p>{/if}
  {#if report}
    {#if !report.enabled}
      <p>{t.notificationsOff}</p>
    {:else}
      <dl class="facts">
        <div><dt>{t.notificationDestination}</dt><dd class="mono">{report.origin}{report.connector ? ` · ${t.notificationConnector}` : ''}</dd></div>
        <div><dt>{t.notificationEvents}</dt><dd>{report.events.join(', ')}</dd></div>
        {#if report.click_milestones.length}<div><dt>{t.notificationMilestones}</dt><dd>{report.click_milestones.map((value) => whole.format(value)).join(', ')}</dd></div>{/if}
        {#if report.expiry_notice_hours !== null}<div><dt>{t.notificationExpiryNotice}</dt><dd>{hours.format(report.expiry_notice_hours)}</dd></div>{/if}
      </dl>
      <div><button class="button" type="button" disabled={working} onclick={sendTest}>{t.notificationSendTest}</button></div>
    {/if}
    <dl class="facts">
      {#each Object.entries(statusLabels) as [key, label] (key)}
        <div><dt>{label}</dt><dd>{whole.format(report.counts[key as Status])}</dd></div>
      {/each}
    </dl>
    {#if report.counts.failed > 0}<p class="notice error">{t.notificationFailedHint}</p>{/if}
    {#if report.items.length}
      <div class="table-wrap">
        <table>
          <thead><tr><th scope="col">{t.notificationEvent}</th><th scope="col">{t.status}</th><th scope="col">{t.notificationTime}</th></tr></thead>
          <tbody>
            {#each report.items as item (item.id)}
              <tr>
                <td class="mono">{item.event}</td>
                <td>
                  {statusLabels[item.status]}{item.attempts ? ` · ${t.notificationAttempts} ${whole.format(item.attempts)}` : ''}
                  {#if item.last_error}<br /><span class="muted">{errorLabels[item.last_error] ?? item.last_error}</span>{/if}
                  {#if item.status === 'pending' && item.attempts > 0}<br /><span class="muted">{t.notificationNextAttempt} {stamp(item.next_attempt_at)}</span>{/if}
                </td>
                <td><time datetime={new Date((item.delivered_at ?? item.occurred_at) * 1000).toISOString()}>{stamp(item.delivered_at ?? item.occurred_at)}</time></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {:else if !error}
    <p role="status">{t.loading}</p>
  {/if}
</section>

<style>
  .wrap { flex-wrap: wrap; }
  .facts { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr)); gap: 8px 24px; margin: 0; }
  .facts dt { color: var(--muted); }
  .facts dd { margin: 0; overflow-wrap: anywhere; font-variant-numeric: tabular-nums; }
</style>
