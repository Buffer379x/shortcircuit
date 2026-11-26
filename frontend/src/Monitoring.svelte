<script lang="ts">
  import { useTimeZone } from './timezone';
  import { onMount } from 'svelte';
  import type { Api, Domains, LinkSearchPage } from './api';
  import { ApiError } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';

  type CheckStatus = 'not_checked' | 'healthy' | 'redirected' | 'warning' | 'unreachable' | 'broken' | 'unknown';
  type Failure = 'blocked' | 'busy' | 'dns' | 'timeout' | 'connection' | 'protocol';
  interface Item {
    id: number;
    host: string;
    slug: string;
    priority: boolean;
    status: CheckStatus;
    last_checked: number | null;
    http_status: number | null;
    elapsed_ms: number | null;
    redirects: number | null;
    failure: Failure | null;
    consecutive_failures: number;
  }
  interface Report { enabled: boolean; items: Item[]; next_after: number | null }
  interface DomainCheck { probe: { status: number; redirects: number; elapsed_ms: number } | null; failure: Failure | null }

  let { api, t, language, domains }: { api: Api; t: Messages; language: Language; domains: Domains | null } = $props();
  let report = $state<Report>();
  let working = $state(false);
  let error = $state<keyof Messages | null>(null);
  let query = $state('');
  let results = $state<LinkSearchPage>();
  let chosenLink = $state<number | undefined>();
  let selectedDomain = $state('');
  let domainResult = $state<DomainCheck | null>(null);

  const statusLabels = $derived<Record<CheckStatus, string>>({
    not_checked: t.monitorNotChecked, healthy: t.monitorHealthy, redirected: t.monitorRedirected,
    warning: t.monitorWarning, unreachable: t.monitorUnreachable, broken: t.monitorBroken, unknown: t.monitorUnknown,
  });
  const failures = $derived<Record<Failure, string>>({
    blocked: t.monitorBlocked, busy: t.monitorBusy, dns: t.monitorDns, timeout: t.monitorTimeout,
    connection: t.monitorConnection, protocol: t.monitorProtocol,
  });
  const readTimeZone = useTimeZone();
  const dateTime = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeZone: readTimeZone(), timeStyle: 'short' }));

  onMount(() => { void load(); });

  async function run(action: () => Promise<void>) {
    if (working) return;
    working = true;
    error = null;
    try {
      await action();
    } catch (issue) {
      error = issue instanceof ApiError && issue.code === 'monitoring_busy' ? 'monitorBusy' : problemMessage(issue);
    } finally {
      working = false;
    }
  }

  async function fetchReport(after?: number) {
    const next = await api<Report>(after ? `/monitoring?after=${after}` : '/monitoring');
    report = after && report ? { ...next, items: [...report.items, ...next.items] } : next;
  }

  function load(after?: number) { return run(() => fetchReport(after)); }

  function search(event: SubmitEvent) {
    event.preventDefault();
    void run(async () => {
      chosenLink = undefined;
      results = await api<LinkSearchPage>('/links/search', 'POST', { query: query.trim() });
    });
  }

  function enroll(id: number, enabled: boolean, priority = false) {
    return run(async () => {
      await api<void>(`/monitoring/links/${id}`, 'PATCH', { enabled, priority });
      chosenLink = undefined;
      await fetchReport();
    });
  }

  function check(id: number) {
    return run(async () => {
      const checked = await api<Item>(`/monitoring/links/${id}/check`, 'POST');
      if (report) report = { ...report, items: report.items.map(item => item.id === id ? checked : item) };
    });
  }

  function checkDomain(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedDomain) return;
    void run(async () => {
      domainResult = null;
      domainResult = await api<DomainCheck>('/monitoring/domain', 'POST', { host: selectedDomain });
    });
  }
</script>

<header class="page-head">
  <div><h1>{t.monitoring}</h1><p>{t.monitorIntro}</p></div>
  <button class="button" type="button" disabled={working} onclick={() => load()}>{t.monitorRefresh}</button>
</header>
{#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
{#if !report && working}
  <p role="status">{t.loading}</p>
{:else if report}
  {#if report.enabled}
    <section class="panel panel-pad stack">
      <h2>{t.monitorAdd}</h2>
      <form class="stack" onsubmit={search}>
        <label>{t.monitorSearch}<input type="search" bind:value={query} maxlength="160" /></label>
        <div><button class="button" type="submit" disabled={working}>{t.monitorSearch}</button></div>
      </form>
      {#if results}
        {#if results.items.length === 0}
          <p role="status">{t.monitorNoMatches}</p>
        {:else}
          <div class="stack">
            <label>{t.links}<select bind:value={chosenLink}>
              <option value={undefined}>{t.monitorChoose}</option>
              {#each results.items as link (link.id)}
                <option value={link.id}>{link.host}/{link.slug}</option>
              {/each}
            </select></label>
            <div><button class="button primary" type="button" disabled={working || !chosenLink}
              onclick={() => chosenLink && enroll(chosenLink, true)}>{t.monitorAdd}</button></div>
          </div>
        {/if}
      {/if}
    </section>
  {:else}
    <p class="notice">{t.monitorOff}</p>
  {/if}
  <section class="panel panel-pad stack">
    <h2>{t.monitorEnrolled}</h2>
    {#each report.items as item (item.id)}
      <article class="monitor-item stack">
        <strong>{item.host}/{item.slug}</strong>
        <span>{statusLabels[item.status]}{item.http_status ? ` · HTTP ${item.http_status}` : ''}{item.elapsed_ms === null ? '' : ` · ${item.elapsed_ms} ms`}</span>
        {#if item.failure}<span>{failures[item.failure]}</span>{/if}
        {#if item.last_checked !== null}
          <time datetime={new Date(item.last_checked * 1000).toISOString()}>{dateTime.format(new Date(item.last_checked * 1000))}</time>
        {/if}
        <div class="monitor-actions">
          <button type="button" class="button" disabled={working || !report.enabled} onclick={() => check(item.id)}>{t.monitorCheck}</button>
          <button type="button" class="button" disabled={working || !report.enabled} aria-pressed={item.priority}
            onclick={() => enroll(item.id, true, !item.priority)}>{t.monitorPriority}</button>
          <button type="button" class="button" disabled={working} onclick={() => enroll(item.id, false)}>{t.monitorRemove}</button>
        </div>
      </article>
    {:else}
      <p>{t.monitorEmpty}</p>
    {/each}
    {#if report.next_after}
      <div><button class="button" type="button" disabled={working} onclick={() => load(report?.next_after ?? undefined)}>{t.monitorMore}</button></div>
    {/if}
  </section>
  {#if report.enabled && domains?.hosts.length}
    <section class="panel panel-pad stack">
      <h2>{t.monitorDomain}</h2>
      <p>{t.monitorDomainHint}</p>
      <form class="stack" onsubmit={checkDomain}>
        <label>{t.domains}<select bind:value={selectedDomain} onchange={() => { domainResult = null; }}>
          <option value="">{t.monitorChoose}</option>
          {#each domains.hosts as host (host)}<option value={host}>{host}</option>{/each}
        </select></label>
        <div><button type="submit" class="button" disabled={!selectedDomain || working}>{t.monitorCheck}</button></div>
      </form>
      {#if domainResult}
        <p role="status">{domainResult.probe ? `HTTP ${domainResult.probe.status} · ${domainResult.probe.elapsed_ms} ms` : domainResult.failure ? failures[domainResult.failure] : t.monitorUnknown}</p>
      {/if}
    </section>
  {/if}
{/if}

<style>
  section { margin-top: var(--space-4, 16px); }
  .monitor-item { padding-block: .75rem; border-top: 1px solid var(--line); overflow-wrap: anywhere; }
  .monitor-actions { display: flex; flex-wrap: wrap; gap: .5rem; }
</style>
