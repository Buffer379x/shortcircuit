<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api } from './api';
  import { ApiError } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';
  import { useTimeZone } from './timezone';

  type Status = 'pending' | 'applied' | 'no_title' | 'failed' | 'skipped';
  type Failure = 'blocked' | 'busy' | 'dns' | 'timeout' | 'connection' | 'protocol' | 'not_html' | 'policy' | 'expired' | 'restored';
  interface Entry {
    link_id: number; host: string; slug: string; status: Status; failure: Failure | null;
    title: string | null; requested_at: number; completed_at: number | null;
  }
  interface Report { enabled: boolean; pending: number; items: Entry[] }

  let { api, t, language }: { api: Api; t: Messages; language: Language } = $props();
  const headingId = $props.id();
  const readTimeZone = useTimeZone();
  const dateTime = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short', timeZone: readTimeZone() }));
  const whole = $derived(new Intl.NumberFormat(language));
  let report = $state<Report>();
  let working = $state(false);
  let error = $state<keyof Messages | null>(null);
  let queued = $state<number | null>(null);

  const statuses = $derived<Record<Status, string>>({
    pending: t.titlePending, applied: t.titleApplied, no_title: t.titleNone, failed: t.titleFailed, skipped: t.titleSkipped,
  });
  const failures = $derived<Record<Failure, string>>({
    blocked: t.monitorBlocked, busy: t.monitorBusy, dns: t.monitorDns, timeout: t.monitorTimeout,
    connection: t.monitorConnection, protocol: t.monitorProtocol, not_html: t.titleNotHtml,
    policy: t.titlePolicy, expired: t.titleExpired, restored: t.titleRestored,
  });

  onMount(() => { void load(); });

  async function load() {
    error = null;
    try {
      report = await api<Report>('/title-lookups');
    } catch (issue) {
      error = problemMessage(issue);
    }
  }

  async function queueUntitled() {
    if (working) return;
    working = true;
    error = null;
    try {
      queued = (await api<{ queued: number }>('/title-lookups', 'POST')).queued;
      await load();
    } catch (issue) {
      error = issue instanceof ApiError && issue.code === 'title_lookup_disabled' ? 'titleDisabled' : problemMessage(issue);
    } finally {
      working = false;
    }
  }
</script>

<section class="panel panel-pad stack" aria-labelledby={headingId} aria-busy={working}>
  <div class="between wrap">
    <h2 id={headingId}>{t.titleLookups}</h2>
    <button class="button" type="button" disabled={working} onclick={load}>{t.monitorRefresh}</button>
  </div>
  <p>{t.titleLookupsIntro}</p>
  {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
  {#if report}
    {#if !report.enabled}
      <p class="notice">{t.titleDisabled}</p>
    {:else}
      <div class="row wrap">
        <button class="button primary" type="button" disabled={working} onclick={queueUntitled}>{t.titleQueueUntitled}</button>
        <span class="muted" role="status">{t.titlePendingCount} {whole.format(report.pending)}{queued === null ? '' : ` · ${t.titleQueued} ${whole.format(queued)}`}</span>
      </div>
    {/if}
    {#if report.items.length}
      <ul class="lookups">
        {#each report.items as item (item.link_id)}
          {@const time = item.completed_at ?? item.requested_at}
          <li class="stack compact">
            <strong class="mono">{item.host}/{item.slug}</strong>
            <span>{statuses[item.status]}{item.failure ? ` · ${failures[item.failure]}` : ''}</span>
            {#if item.title}<span>{item.title}</span>{/if}
            <time class="muted" datetime={new Date(time * 1000).toISOString()}>{dateTime.format(new Date(time * 1000))}</time>
          </li>
        {/each}
      </ul>
    {:else if report.enabled}
      <p>{t.titleNoLookups}</p>
    {/if}
  {:else if !error}
    <p role="status">{t.loading}</p>
  {/if}
</section>

<style>
  .wrap { flex-wrap: wrap; }
  .lookups { margin: 0; padding: 0; list-style: none; }
  .lookups li { padding-block: .75rem; border-top: 1px solid var(--line); overflow-wrap: anywhere; }
</style>
