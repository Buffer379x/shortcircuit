<script lang="ts">
  // Health: destination checks for monitored links (problems first), enrollment, priority, manual
  // and batch checks, and checks of the public domains. Every request goes through the server's
  // outbound policy; this page only starts checks and shows their results.
  import { onMount } from 'svelte';
  import { ApiError, type CatalogLink, type LinkSearchPage, type MonitorStatus } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import type { IconName } from '../lib/icons';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import ShortUrl from '../lib/ui/ShortUrl.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import { app } from '../lib/app.svelte';
  import { tr } from '../lib/locale.svelte';
  import { health, healthState, type Health, type StateView } from '../lib/status';
  import { ago, num, splitUrl, when } from '../lib/fmt';
  import { href, route, setParams } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tip } from '../lib/tooltip';

  interface Item {
    id: number;
    host: string;
    slug: string;
    priority: boolean;
    status: MonitorStatus;
    last_checked: number | null;
    http_status: number | null;
    elapsed_ms: number | null;
    redirects: number | null;
    failure: string | null;
    consecutive_failures: number;
  }
  interface Report { enabled: boolean; items: Item[]; next_after: number | null }
  interface DomainCheck { probe: { status: number; redirects: number; elapsed_ms: number } | null; failure: string | null }
  type Filter = Health | 'all';

  const failures: Record<string, keyof Messages> = {
    blocked: 'monitorBlocked', busy: 'monitorBusy', dns: 'monitorDns', timeout: 'monitorTimeout',
    connection: 'monitorConnection', protocol: 'monitorProtocol',
  };
  const order: Record<Health, number> = { down: 0, warning: 1, redirect: 2, pending: 3, up: 4, unchecked: 5 };
  const tiles: [Filter, keyof Messages, IconName][] = [
    ['all', 'insMonitored', 'activity'], ['down', 'healthDown', 'circle-x'], ['warning', 'monitorWarning', 'triangle-alert'],
    ['redirect', 'insRedirecting', 'corner-down-right'], ['up', 'healthUp', 'circle-check'], ['pending', 'healthPending', 'clock'],
  ];
  const filters: Filter[] = ['all', 'down', 'warning', 'redirect', 'up', 'pending'];

  const initialFilter = route.params.get('filter') as Filter | null;
  let filter = $state<Filter>(initialFilter && filters.includes(initialFilter) ? initialFilter : 'all');
  let query = $state(route.params.get('q') ?? '');

  let report = $state.raw<Report | null>(null);
  let loadError = $state<keyof Messages | null>(null);
  /** The action in progress. One at a time, as before, so checks and enrollment never race. */
  let working = $state<string | null>(null);
  let domainResults = $state.raw<Record<string, DomainCheck>>({});

  let adding = $state(false);
  let search = $state('');
  let results = $state.raw<CatalogLink[] | null>(null);
  let resultsMore = $state(false);
  let searchError = $state<keyof Messages | null>(null);
  let searching = $state(false);
  let picks = $state<number[]>([]);
  let searchRequest: AbortController | undefined;
  /** Title and destination per link (GET /links/{id}). Optional: rows fall back to the short URL. */
  let details = $state.raw<Record<number, CatalogLink>>({});
  const detailRequest = new AbortController();

  onMount(() => {
    void load();
    return () => {
      searchRequest?.abort();
      detailRequest.abort();
    };
  });

  /** Loads missing details for the listed links, at most four requests at a time. */
  async function loadDetails(ids: number[]) {
    const queue = ids.filter((id) => !details[id]);
    const worker = async () => {
      for (let id = queue.shift(); id !== undefined && !detailRequest.signal.aborted; id = queue.shift()) {
        try {
          const link = await app.api<CatalogLink>(`/links/${id}`, 'GET', undefined, 'json', detailRequest.signal);
          if (link?.target) details = { ...details, [id]: link };
        } catch {
          /* The row keeps its short URL. */
        }
      }
    };
    await Promise.all(Array.from({ length: 4 }, worker));
  }

  function message(error: unknown): keyof Messages {
    if (error instanceof ApiError && error.code === 'monitoring_busy') return 'monitorBusy';
    if (error instanceof ApiError && error.code === 'monitoring_link_inactive') return 'monitorLinkInactive';
    return problemMessage(error);
  }

  async function run(key: string, action: () => Promise<void>) {
    if (working) return;
    working = key;
    try {
      await action();
    } catch (error) {
      toast(tr(message(error)), { tone: 'error' });
    } finally {
      working = null;
    }
  }

  async function fetchReport(after?: number) {
    const next = await app.api<Report>(after ? `/monitoring?after=${after}` : '/monitoring');
    report = after && report ? { ...next, items: [...report.items, ...next.items] } : next;
    void loadDetails(next.items.map((item) => item.id));
  }

  async function load(after?: number) {
    if (working) return;
    working = after ? 'more' : 'load';
    loadError = null;
    try {
      await fetchReport(after);
    } catch (error) {
      loadError = message(error);
    } finally {
      working = null;
    }
  }

  function setFilter(next: Filter) {
    filter = next;
    setParams({ filter: next === 'all' ? null : next });
  }

  function setQuery(value: string) {
    query = value;
    setParams({ q: value.trim() || null });
  }

  function patchItem(id: number, change: Partial<Item>) {
    if (report) report = { ...report, items: report.items.map((item) => (item.id === id ? { ...item, ...change } : item)) };
  }

  function queueChecks() {
    return run('all', async () => {
      const result = await app.api<{ queued: number }>('/monitoring/check', 'POST');
      toast(tr('insChecksQueued', { count: num(result.queued) }), { tone: 'info' });
    });
  }

  function check(item: Item) {
    return run(`check:${item.id}`, async () => {
      const checked = await app.api<Item>(`/monitoring/links/${item.id}/check`, 'POST');
      patchItem(item.id, checked);
      toast(tr('insCheckDone'), { tone: 'info', body: `${item.host}/${item.slug} · ${healthState(health(checked.status)).label}` });
    });
  }

  function setPriority(item: Item, input: HTMLInputElement) {
    const priority = input.checked;
    if (working) {
      input.checked = item.priority;
      return;
    }
    void run(`priority:${item.id}`, async () => {
      try {
        await app.api<void>(`/monitoring/links/${item.id}`, 'PATCH', { enabled: true, priority });
      } catch (error) {
        input.checked = item.priority;
        throw error;
      }
      patchItem(item.id, { priority });
      toast(priority ? tr('insPriorityOn') : tr('insPriorityOff'), { body: `${item.host}/${item.slug}` });
    });
  }

  function stop(item: Item) {
    return run(`stop:${item.id}`, async () => {
      await app.api<void>(`/monitoring/links/${item.id}`, 'PATCH', { enabled: false });
      if (report) report = { ...report, items: report.items.filter((entry) => entry.id !== item.id) };
      toast(tr('insMonitoringStopped'), { body: `${item.host}/${item.slug}` });
    });
  }

  function checkDomain(host: string) {
    return run(`domain:${host}`, async () => {
      const result = await app.api<DomainCheck>('/monitoring/domain', 'POST', { host });
      domainResults = { ...domainResults, [host]: result };
    });
  }

  function openAdd() {
    picks = [];
    search = '';
    adding = true;
    void findLinks();
  }

  async function findLinks(event?: SubmitEvent) {
    event?.preventDefault();
    searchRequest?.abort();
    const request = (searchRequest = new AbortController());
    searching = true;
    searchError = null;
    try {
      const page = await app.api<LinkSearchPage>('/links/search', 'POST', { query: search.trim() }, 'json', request.signal);
      if (request.signal.aborted) return;
      results = page.items.filter((link) => !link.archived);
      resultsMore = page.next_cursor !== null;
    } catch (error) {
      if (!request.signal.aborted) searchError = problemMessage(error);
    } finally {
      if (!request.signal.aborted) searching = false;
    }
  }

  function togglePick(id: number) {
    picks = picks.includes(id) ? picks.filter((entry) => entry !== id) : [...picks, id];
  }

  function enrollPicked() {
    const ids = [...picks];
    if (!ids.length) return;
    void run('enroll', async () => {
      let done = 0;
      try {
        for (const id of ids) {
          await app.api<void>(`/monitoring/links/${id}`, 'PATCH', { enabled: true, priority: false });
          done++;
        }
      } finally {
        if (done) {
          await fetchReport().catch(() => undefined);
          toast(tr('insEnrolled', { count: num(done) }));
        }
      }
      adding = false;
    });
  }

  const items = $derived(report?.items ?? []);
  const monitoredIds = $derived(new Set(items.map((item) => item.id)));
  const counts = $derived.by(() => {
    const result: Record<Filter, number> = { all: items.length, down: 0, warning: 0, redirect: 0, up: 0, pending: 0, unchecked: 0 };
    for (const item of items) result[health(item.status)]++;
    return result;
  });
  const rows = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    return items
      .filter((item) => (filter === 'all' || health(item.status) === filter)
        && (!needle || `${item.host}/${item.slug} ${details[item.id]?.title ?? ''} ${details[item.id]?.target ?? ''}`.toLowerCase().includes(needle)))
      .sort((a, b) => order[health(a.status)] - order[health(b.status)] || a.id - b.id);
  });
  const shownTiles = $derived(tiles.filter(([key]) => key === 'all' || key === filter || counts[key] > 0 || key === 'down' || key === 'redirect' || key === 'up'));
  const enabled = $derived(report?.enabled ?? false);

  function detail(item: Item) {
    const parts: string[] = [];
    if (item.status === 'broken' || item.status === 'unknown') parts.push(tr(item.status === 'broken' ? 'monitorBroken' : 'monitorUnknown'));
    if (item.status === 'not_checked') parts.push(tr('monitorNotChecked'));
    if (item.failure) parts.push(tr(failures[item.failure] ?? 'monitorUnknown'));
    if (item.http_status) parts.push(`HTTP ${item.http_status}`);
    if (item.redirects) parts.push(tr('insRedirectCount', { count: num(item.redirects) }));
    if (item.consecutive_failures > 1) parts.push(tr('insFailuresInRow', { count: num(item.consecutive_failures) }));
    return parts.join(' · ');
  }

  function domainState(result: DomainCheck): StateView {
    if (result.probe) {
      return result.probe.status < 400
        ? { tone: 'success', icon: 'circle-check', label: `HTTP ${result.probe.status}` }
        : { tone: 'warning', icon: 'triangle-alert', label: `HTTP ${result.probe.status}` };
    }
    return { tone: 'error', icon: 'circle-x', label: tr(result.failure ? failures[result.failure] ?? 'monitorUnknown' : 'monitorUnknown') };
  }
</script>

<div class="page">
  <PageHeader title={tr('navHealth')} description={tr('insHealthDescription')}>
    {#snippet actions()}
      <button class="btn" type="button" data-variant="ghost" data-icon-only aria-label={tr('monitorRefresh')} use:tip={tr('monitorRefresh')} disabled={!!working} onclick={() => load()}>
        {#if working === 'load' && report}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="rotate-ccw" />{/if}
      </button>
      {#if enabled && items.length}
        <button class="btn" type="button" disabled={!!working} onclick={queueChecks}>
          {#if working === 'all'}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="refresh-cw" />{/if}{tr('insCheckAll')}
        </button>
      {/if}
      <button class="btn" type="button" data-variant="primary" disabled={!enabled || !!working} onclick={openAdd}><Icon name="plus" />{tr('insMonitorLinks')}</button>
    {/snippet}
  </PageHeader>

  {#if loadError}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr(loadError)}</p></div>
      <button class="btn" type="button" data-size="sm" onclick={() => load()}>{tr('retry')}</button>
    </div>
  {/if}

  {#if report && !report.enabled}
    <div class="notice" data-tone="warning"><Icon name="circle-pause" /><div><p>{tr('monitorOff')}</p></div></div>
  {/if}

  {#if !report && !loadError}
    <div class="panel summary-loading skeleton" aria-hidden="true"></div>
    <span class="visually-hidden" role="status">{tr('loading')}</span>
  {:else if report}
    <section class="panel summary" aria-label={tr('insSummary')}>
      {#each shownTiles as [key, label, icon] (key)}
        <button class="sum" type="button" aria-pressed={filter === key} data-tone={key} onclick={() => setFilter(key)}>
          <span class="sum-label"><Icon name={icon} />{tr(label)}</span>
          <b class="num">{num(counts[key])}</b>
        </button>
      {/each}
    </section>

    <section class="panel">
      <header class="panel-head">
        <div class="panel-title">
          <h2>{filter === 'all' ? tr('insAllMonitored') : filter === 'redirect' ? tr('insRedirecting') : healthState(filter).label}</h2>
          <p>{tr('insProblemsFirst')}</p>
        </div>
        <span class="spacer"></span>
        <div class="search filter-search">
          <Icon name="search" />
          <input class="input" type="search" placeholder={tr('insFilterPlaceholder')} aria-label={tr('insFilterLabel')} maxlength="160"
            value={query} oninput={(event) => setQuery(event.currentTarget.value)} />
        </div>
      </header>
      {#if rows.length}
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th scope="col" class="c-link">{tr('insColLink')}</th>
                <th scope="col" class="c-dest">{tr('insColDestination')}</th>
                <th scope="col">{tr('insColResult')}</th>
                <th scope="col" class="r c-resp">{tr('insColResponse')}</th>
                <th scope="col" class="r c-checked">{tr('insColChecked')}</th>
                <th scope="col">{tr('insColPriority')}</th>
                <th scope="col" class="c-actions"><span class="visually-hidden">{tr('insColActions')}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each rows as item (item.id)}
                {@const name = `${item.host}/${item.slug}`}
                {@const info = detail(item)}
                {@const link = details[item.id]}
                <tr aria-busy={working?.endsWith(`:${item.id}`) || undefined}>
                  <td class="c-link">
                    <a class="cell-link" href={href('links', item.id, { tab: 'health' })}>
                      {#if link?.title}<span class="truncate">{link.title}</span>{/if}<ShortUrl host={item.host} slug={item.slug} />
                    </a>
                  </td>
                  <td class="c-dest">
                    {#if link}{@const d = splitUrl(link.target)}<span class="dest dest-cell"><b>{d.host}</b>{d.rest}</span>{:else}<span class="muted">—</span>{/if}
                  </td>
                  <td>
                    <div class="res">
                      <Badge state={healthState(health(item.status))} quiet />
                      {#if info}<span class="muted">{info}</span>{/if}
                    </div>
                  </td>
                  <td class="r num c-resp">{item.elapsed_ms === null ? '—' : `${num(item.elapsed_ms)} ms`}</td>
                  <td class="r muted c-checked nowrap">
                    {#if item.last_checked !== null}
                      <time datetime={new Date(item.last_checked * 1000).toISOString()} title={when(item.last_checked)}>{ago(item.last_checked)}</time>
                    {:else}{tr('never')}{/if}
                  </td>
                  <td>
                    <label class="check" use:tip={tr('monitorPriority')}>
                      <input class="switch" type="checkbox" checked={item.priority} disabled={!enabled || !!working}
                        aria-label={tr('insPriorityFor', { name })} onchange={(event) => setPriority(item, event.currentTarget)} />
                    </label>
                  </td>
                  <td class="c-actions">
                    <div class="row-actions">
                      <button class="btn reveal" type="button" data-variant="ghost" data-size="sm" data-icon-only disabled={!enabled || !!working}
                        aria-label={tr('insCheckNowFor', { name })} use:tip={tr('monitorCheck')} onclick={() => check(item)}>
                        {#if working === `check:${item.id}`}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="refresh-cw" />{/if}
                      </button>
                      <Menu label={tr('moreActionsFor', { name })} icon="ellipsis" size="sm">
                        {#snippet children(close)}
                          <a class="menu-item" role="menuitem" href={href('links', item.id, { tab: 'health' })} onclick={close}><Icon name="history" />{tr('insOpenDetails')}</a>
                          <div class="menu-sep"></div>
                          <button class="menu-item" type="button" role="menuitem" disabled={!!working} onclick={() => { close(); void stop(item); }}><Icon name="circle-pause" />{tr('monitorRemove')}</button>
                        {/snippet}
                      </Menu>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if items.length}
        <Empty icon="circle-check" title={tr('insNothingHere')}>{tr('insNoResult')}</Empty>
      {:else}
        <Empty icon="activity" title={tr('monitorEmpty')}>{enabled ? tr('insEmptyHint') : tr('monitorOff')}</Empty>
      {/if}
      {#if report.next_after}
        <footer class="panel-foot pager">
          <span class="muted">{tr('insLoadedOnly', { count: num(items.length) })}</span>
          <span class="spacer"></span>
          <button class="btn" type="button" data-size="sm" disabled={!!working} onclick={() => load(report?.next_after ?? undefined)}>
            {#if working === 'more'}<span class="spinner" aria-hidden="true"></span>{/if}{tr('monitorMore')}
          </button>
        </footer>
      {/if}
    </section>

    {#if report.enabled && app.domains?.hosts.length}
      <section class="panel">
        <header class="panel-head"><div class="panel-title"><h2>{tr('insPublicDomains')}</h2><p>{tr('monitorDomainHint')}</p></div></header>
        <div class="table-wrap">
          <table class="table" data-density="compact">
            <thead>
              <tr>
                <th scope="col">{tr('domain')}</th>
                <th scope="col">{tr('insColResult')}</th>
                <th scope="col" class="r">{tr('insColResponse')}</th>
                <th scope="col" class="c-actions"><span class="visually-hidden">{tr('insColActions')}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each app.domains.hosts as host (host)}
                {@const result = domainResults[host]}
                <tr aria-busy={working === `domain:${host}` || undefined}>
                  <td class="mono">{host}</td>
                  <td class="nowrap">{#if result}<Badge state={domainState(result)} />{:else}<span class="muted">{tr('monitorNotChecked')}</span>{/if}</td>
                  <td class="r num">{result?.probe ? `${num(result.probe.elapsed_ms)} ms` : '—'}</td>
                  <td class="c-actions">
                    <button class="btn nowrap" type="button" data-size="sm" disabled={!!working} aria-label={tr('insCheckNowFor', { name: host })} onclick={() => checkDomain(host)}>
                      {#if working === `domain:${host}`}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="refresh-cw" />{/if}{tr('monitorCheck')}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}
  {/if}
</div>

<Dialog bind:open={adding} title={tr('insMonitorLinks')} description={tr('insMonitorDialogHint')} width={520} busy={working === 'enroll'}>
  <form class="find" onsubmit={findLinks} role="search">
    <div class="search find-input">
      <Icon name="search" />
      <input class="input" type="search" bind:value={search} maxlength="160" placeholder={tr('monitorSearch')} aria-label={tr('monitorSearch')}
        aria-invalid={searchError ? 'true' : undefined} aria-describedby={searchError ? 'monitor-search-error' : undefined} />
    </div>
    <button class="btn" type="submit" disabled={searching}>{#if searching}<span class="spinner" aria-hidden="true"></span>{/if}{tr('insSearch')}</button>
  </form>
  {#if searchError}<p class="field-error" id="monitor-search-error" role="alert"><Icon name="circle-alert" />{tr(searchError)}</p>{/if}
  {#if results}
    {#if results.length}
      <ul class="pick" aria-busy={searching}>
        {#each results as link (link.id)}
          {@const monitored = monitoredIds.has(link.id) || !!link.monitor}
          <li>
            <label class="check">
              <input class="checkbox" type="checkbox" checked={monitored || picks.includes(link.id)} disabled={monitored || working === 'enroll'} onchange={() => togglePick(link.id)} />
              <span class="pick-name">
                <span class="truncate">{link.title || link.slug}</span>
                <small class="mono">{link.host}/{link.slug}</small>
              </span>
              {#if monitored}<span class="badge" data-tone="muted">{tr('insAlreadyMonitored')}</span>{/if}
            </label>
          </li>
        {/each}
      </ul>
      {#if resultsMore}<p class="muted more-hint">{tr('insMoreResults')}</p>{/if}
    {:else if !searching}
      <p class="muted" role="status">{tr('monitorNoMatches')}</p>
    {/if}
  {/if}
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={working === 'enroll'} onclick={() => (adding = false)}>{tr('cancel')}</button>
    <button class="btn" type="button" data-variant="primary" disabled={!picks.length || !!working} onclick={enrollPicked}>
      {#if working === 'enroll'}<span class="spinner" aria-hidden="true"></span>{/if}
      {picks.length ? tr('insMonitorCount', { count: num(picks.length) }) : tr('insMonitorLinks')}
    </button>
  {/snippet}
</Dialog>

<style>
  .summary {
    display: grid;
    grid-auto-columns: minmax(0, 1fr);
    grid-auto-flow: column;
    overflow: hidden;
  }
  .summary-loading {
    height: 86px;
  }
  .sum {
    display: grid;
    gap: 6px;
    padding: var(--space-4) var(--space-5);
    border: 0;
    border-right: 1px solid var(--border-subtle);
    background: transparent;
    text-align: left;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .sum:last-child {
    border-right: 0;
  }
  .sum:hover {
    background: var(--surface-hover);
  }
  .sum:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .sum[aria-pressed='true'] {
    background: var(--surface-raised);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .sum-label {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .sum-label :global(svg) {
    width: 15px;
    height: 15px;
    color: var(--text-muted);
  }
  [data-tone='down'] .sum-label :global(svg) {
    color: var(--error);
  }
  [data-tone='redirect'] .sum-label :global(svg),
  [data-tone='warning'] .sum-label :global(svg) {
    color: var(--warning);
  }
  [data-tone='up'] .sum-label :global(svg) {
    color: var(--success);
  }
  .sum b {
    font-size: var(--text-2xl);
    line-height: var(--leading-2xl);
    font-weight: var(--weight-semibold);
  }
  .filter-search {
    width: min(280px, 100%);
  }
  .c-link {
    min-width: 200px;
  }
  .cell-link {
    display: grid;
    min-width: 0;
    max-width: 280px;
    color: var(--text);
    font-weight: var(--weight-medium);
    text-decoration: none;
  }
  .cell-link:hover > .truncate {
    color: var(--accent);
  }
  .cell-link :global(.short-url) {
    font-size: var(--text-xs);
    font-weight: var(--weight-regular);
  }
  .dest-cell {
    max-width: 300px;
  }
  .res {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .nowrap {
    white-space: nowrap;
  }
  .pager {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-xs);
  }
  .find {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .find-input {
    flex: 1;
  }
  .pick {
    display: grid;
    gap: 2px;
    max-height: 360px;
    overflow-y: auto;
  }
  .pick li {
    padding: var(--space-2) 4px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .pick label {
    width: 100%;
  }
  .pick-name {
    display: grid;
    flex: 1;
    min-width: 0;
  }
  .pick small {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .more-hint {
    margin-top: var(--space-3);
    font-size: var(--text-xs);
  }
  @media (max-width: 1100px) {
    .c-dest,
    .c-resp {
      display: none;
    }
  }
  @media (max-width: 900px) {
    .summary {
      grid-auto-flow: row;
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
    .sum {
      border-bottom: 1px solid var(--border-subtle);
    }
  }
  @media (max-width: 640px) {
    .summary {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .c-checked {
      display: none;
    }
    .c-link {
      min-width: 150px;
    }
    .panel-head .filter-search {
      width: 100%;
    }
  }
</style>
