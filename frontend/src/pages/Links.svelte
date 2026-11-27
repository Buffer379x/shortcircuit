<script lang="ts">
  // The link list: saved views, search, filters, sorting, columns, selection with bulk actions,
  // and cursor pagination. Rows become cards in narrow containers. The Tags tab shares the page.
  import { onDestroy, tick, untrack } from 'svelte';
  import { linkSorts } from '../api';
  import type { BulkLinkResult, CatalogLink, Link, LinkCursor, LinkOrganization, LinkSearch, LinkSearchPage, LinkSort, LinkState, LinkView, TagMatch } from '../api';
  import { downloadFile, problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Icon from '../lib/Icon.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import HealthIcon from '../lib/ui/HealthIcon.svelte';
  import TagChip from '../lib/ui/TagChip.svelte';
  import TagInput from '../lib/ui/TagInput.svelte';
  import ShortUrl from '../lib/ui/ShortUrl.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import LinkPreview from '../lib/ui/LinkPreview.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import Sparkline from '../lib/charts/Sparkline.svelte';
  import Tags from './Tags.svelte';
  import { createSeed, linkCache } from '../features/LinkForm.svelte';
  import { linkState, health } from '../lib/status';
  import { app, loadPreviewSetting, loadTags, shortUrl, tagColor } from '../lib/app.svelte';
  import { num, ago, day, splitUrl } from '../lib/fmt';
  import { route, href, setParams } from '../lib/router.svelte';
  import { overlays, openCreate, linksChanged } from '../lib/overlays.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tr } from '../lib/locale.svelte';
  import { tip } from '../lib/tooltip';

  // Thumbnails appear only while preview images are on.
  void loadPreviewSetting();

  type View = 'all' | 'favorites' | 'attention' | 'archived';
  type Column = 'dest' | 'tags' | 'visits' | 'trend' | 'status' | 'health' | 'last';
  const VIEWS: [View, keyof Messages][] = [['all', 'lnkViewAll'], ['favorites', 'favoriteLinks'], ['attention', 'lnkViewAttention'], ['archived', 'stateArchived']];
  const API_VIEW: Record<Exclude<View, 'attention'>, LinkView> = { all: 'unarchived', favorites: 'favorites', archived: 'archived' };
  const COLUMNS: [Column, keyof Messages][] = [
    ['dest', 'lnkColDestination'], ['tags', 'tags'], ['visits', 'lnkColVisits'], ['trend', 'lnkColTrend'],
    ['status', 'status'], ['health', 'navHealth'], ['last', 'lnkColLast'],
  ];
  const SORT_LABELS = {
    newest: 'sortNewest', oldest: 'sortOldest', title_asc: 'sortTitleAscending', title_desc: 'sortTitleDescending',
    slug_asc: 'sortSlugAscending', slug_desc: 'sortSlugDescending', host_asc: 'sortHostAscending', host_desc: 'sortHostDescending',
    target_asc: 'sortTargetAscending', target_desc: 'sortTargetDescending', enabled_first: 'sortEnabledFirst', disabled_first: 'sortDisabledFirst',
    visits_desc: 'lnkSortVisitsDesc', visits_asc: 'lnkSortVisitsAsc', last_visit_desc: 'lnkSortLastVisitDesc', last_visit_asc: 'lnkSortLastVisitAsc',
  } as const satisfies Record<LinkSort, keyof Messages>;
  // The live states the server filters by, in the order of the status filter.
  const STATES: [LinkState, keyof Messages][] = [
    ['active', 'stateActive'], ['scheduled', 'stateScheduled'], ['expired', 'stateExpired'], ['disabled', 'stateDisabled'], ['exhausted', 'stateExhausted'],
  ];
  const MAX_SELECTION = 50;
  const MAX_TAG_FILTERS = 20;

  function stored<T>(key: string, fallback: T, valid: (v: unknown) => boolean): T {
    try {
      const raw = localStorage.getItem(key);
      const value = raw == null ? fallback : JSON.parse(raw);
      return valid(value) ? value : fallback;
    } catch {
      return fallback;
    }
  }
  function store(key: string, value: unknown) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* The choice still applies to this page. */
    }
  }

  const p = route.params;
  const paramView = p.get('view') as View;
  let view = $state<View>(VIEWS.some(([v]) => v === paramView) ? paramView : 'all');
  let q = $state(p.get('q') ?? '');
  let status = $state<LinkState | ''>(STATES.find(([s]) => s === p.get('status'))?.[0] ?? '');
  let host = $state(p.get('host') ?? '');
  let tagFilter = $state<string[]>(p.get('tags')?.split(',').filter(Boolean).slice(0, MAX_TAG_FILTERS) ?? []);
  let tagMatch = $state<TagMatch>(p.get('match') === 'any' ? 'any' : 'all');
  let sort = $state<LinkSort>((linkSorts as readonly string[]).includes(p.get('sort') ?? '') ? (p.get('sort') as LinkSort) : 'newest');
  let density = $state<'comfortable' | 'compact'>(stored('shortcircuit-links-density', 'comfortable', (v) => v === 'compact' || v === 'comfortable'));
  let hidden = $state<Column[]>(stored('shortcircuit-links-columns', [], (v) => Array.isArray(v)));
  let perPage = $state<number>(stored('shortcircuit-links-rows', 25, (v) => v === 10 || v === 25 || v === 50));
  let page = $state(0);
  let filtersOpen = $state(false);

  let items = $state<CatalogLink[]>([]);
  let cursor = $state.raw<LinkCursor | null>(null);
  let loading = $state(false);
  let initialized = $state(false);
  let loadError = $state<keyof Messages | null>(null);
  let selected = $state.raw(new Set<string>());
  let updating = $state<number | null>(null);
  let bulkBusy = $state(false);
  let bulkError = $state<keyof Messages | null>(null);
  let counts = $state<{ archived: number | null; attention: number | null; favorites: string | null }>({ archived: null, attention: null, favorites: null });

  let viewGroup = $state<HTMLElement>();
  let removing = $state<CatalogLink | null>(null);
  let removeBusy = $state(false);
  let removeError = $state<keyof Messages | null>(null);
  let bulkDelete = $state<CatalogLink[] | null>(null);
  let bulkConfirm = $state('');
  let tagDialog = $state<'add_tags' | 'remove_tags' | null>(null);
  let bulkTags = $state<string[]>([]);

  const isTags = route.page === 'tags';
  const show = (c: Column) => !hidden.includes(c);
  const filtersActive = $derived(Boolean(q.trim() || status || host || tagFilter.length));
  const pageRows = $derived(items.slice(page * perPage, (page + 1) * perPage));
  const selectedLinks = $derived(items.filter((l) => selected.has(l.selection_key)));
  const pageSelectable = $derived(pageRows.slice(0, MAX_SELECTION));
  const allSelected = $derived(pageSelectable.length > 0 && pageSelectable.every((l) => selected.has(l.selection_key)));
  const someSelected = $derived(!allSelected && pageRows.some((l) => selected.has(l.selection_key)));
  const hosts = $derived(Object.keys(app.domains?.origins ?? {}).sort());
  const allCount = $derived(app.counts.links);

  $effect(() => {
    if (isTags) return;
    setParams({
      view: view === 'all' ? null : view,
      q: q.trim() || null,
      status: status || null,
      host: host || null,
      tags: tagFilter.join(',') || null,
      match: tagFilter.length > 1 && tagMatch === 'any' ? 'any' : null,
      sort: sort === 'newest' ? null : sort,
    });
  });
  $effect(() => store('shortcircuit-links-density', density));
  $effect(() => store('shortcircuit-links-columns', hidden));
  $effect(() => store('shortcircuit-links-rows', perPage));

  // ---------- Loading ----------
  let queryKey = '';
  let currentCriteria: LinkSearch | null = null;
  let countsLoaded = false;
  let loadedPages = 0;
  let current: AbortController | null = null;
  let retry: (() => void) | null = null;
  onDestroy(() => current?.abort());

  $effect(() => {
    if (isTags) return;
    void overlays.linksVersion;
    const criteria: LinkSearch | null = view === 'attention' ? null : {
      query: q.trim() || undefined,
      host: host || undefined,
      tags: tagFilter.length ? [...tagFilter] : undefined,
      tag_match: tagFilter.length > 1 ? tagMatch : undefined,
      state: status || undefined,
      view: API_VIEW[view as Exclude<View, 'attention'>],
      sort,
    };
    const key = JSON.stringify([view, criteria, view === 'attention' ? [q, host, status, tagFilter, tagMatch] : null]);
    return untrack(() => {
      const refresh = key === queryKey;
      const depth = refresh ? loadedPages : 0;
      current?.abort();
      const pending = new AbortController();
      current = pending;
      queryKey = key;
      currentCriteria = criteria;
      if (!refresh) {
        items = [];
        cursor = null;
        page = 0;
        initialized = false;
        loadedPages = 0;
        selected = new Set();
      }
      loading = true;
      loadError = null;
      const timer = setTimeout(() => {
        if (criteria) void loadPages(criteria, null, pending.signal, Math.max(1, depth));
        else void loadAttention(pending.signal);
        if (refresh || !countsLoaded) void loadCounts(pending.signal);
      }, refresh ? 0 : 150);
      return () => {
        clearTimeout(timer);
        pending.abort();
      };
    });
  });

  function remember(list: CatalogLink[]) {
    for (const l of list) linkCache.set(l.id, l);
  }

  async function loadPages(criteria: LinkSearch, after: LinkCursor | null, signal: AbortSignal, pages = 1) {
    loading = true;
    loadError = null;
    retry = null;
    try {
      const collected = after ? [...items] : [];
      const seen = new Set(collected.map((l) => l.id));
      let next = after;
      let depth = after ? loadedPages : 0;
      for (let i = 0; i < pages; i++) {
        const result = await app.api<LinkSearchPage>('/links/search', 'POST', { ...criteria, after: next ?? undefined }, 'json', signal);
        if (signal.aborted) return;
        for (const l of result.items) if (!seen.has(l.id)) { collected.push(l); seen.add(l.id); }
        depth++;
        next = result.next_cursor;
        if (!next) break;
      }
      remember(collected);
      items = collected;
      selected = new Set(collected.map((l) => l.selection_key).filter((k) => selected.has(k)));
      cursor = next;
      loadedPages = depth;
      initialized = true;
      if (page * perPage >= Math.max(1, items.length)) page = Math.max(0, Math.ceil(items.length / perPage) - 1);
    } catch (error) {
      if (!signal.aborted) {
        loadError = problemMessage(error);
        retry = () => void loadPages(criteria, after, signal, pages);
      }
    } finally {
      if (!signal.aborted) loading = false;
    }
  }

  // Needs attention comes from the overview: unreachable destinations, links ending within 14 days,
  // and links that used most of their visit limit. Filters apply to that short list here.
  async function loadAttention(signal: AbortSignal) {
    loading = true;
    loadError = null;
    retry = null;
    try {
      const overview = await app.api<{ attention?: { id: number }[] }>('/overview', 'GET', undefined, 'json', signal);
      const ids = (overview.attention ?? []).map((a) => a.id);
      const found = await Promise.all(ids.map((id) =>
        app.api<CatalogLink>(`/links/${id}`, 'GET', undefined, 'json', signal).catch(() => linkCache.get(id) ?? null)));
      if (signal.aborted) return;
      const text = q.trim().toLowerCase();
      const list = found.filter((l): l is CatalogLink => l != null).filter((l) =>
        (!host || l.host === host) && (!status || l.state === status)
        && (!tagFilter.length || (tagMatch === 'all' ? tagFilter.every((t) => l.tags.includes(t)) : tagFilter.some((t) => l.tags.includes(t))))
        && (!text || [l.title, l.description, l.slug, l.host, l.target, `${l.host}/${l.slug}`, ...l.tags].some((v) => (v ?? '').toLowerCase().includes(text))));
      remember(list);
      items = list;
      cursor = null;
      loadedPages = 1;
      initialized = true;
      selected = new Set(list.map((l) => l.selection_key).filter((k) => selected.has(k)));
    } catch (error) {
      if (!signal.aborted) {
        loadError = problemMessage(error);
        retry = () => void loadAttention(signal);
      }
    } finally {
      if (!signal.aborted) loading = false;
    }
  }

  async function loadCounts(signal: AbortSignal) {
    try {
      const overview = await app.api<{ links: { total: number; archived?: number }; attention?: unknown[] }>('/overview', 'GET', undefined, 'json', signal);
      counts.archived = overview.links.archived ?? null;
      counts.attention = overview.attention ? overview.attention.length : null;
      app.counts.links = overview.links.total - (overview.links.archived ?? 0);
      const favorites = await app.api<LinkSearchPage>('/links/search', 'POST', { view: 'favorites', sort: 'newest' }, 'json', signal);
      counts.favorites = num(favorites.items.length) + (favorites.next_cursor ? '+' : '');
      countsLoaded = true;
    } catch {
      /* The view counts are optional. */
    }
  }

  function loadMore(): Promise<void> | undefined {
    if (loading || !cursor || !current || !currentCriteria) return;
    return loadPages(currentCriteria, cursor, current.signal);
  }

  async function nextPage() {
    if ((page + 1) * perPage >= items.length) await loadMore();
    if ((page + 1) * perPage < items.length) page++;
  }

  // ---------- Filters ----------
  function clearFilters() {
    q = '';
    status = '';
    host = '';
    tagFilter = [];
    tagMatch = 'all';
  }
  function toggleTag(name: string) {
    if (tagFilter.includes(name)) tagFilter = tagFilter.filter((t) => t !== name);
    else if (tagFilter.length < MAX_TAG_FILTERS) tagFilter = [...tagFilter, name];
  }
  // A header applies its first sort, then toggles between both directions.
  function sortBy(first: LinkSort, second: LinkSort) {
    sort = sort === first ? second : first;
  }
  const ariaSort = (asc: LinkSort, desc: LinkSort) => (sort === asc ? 'ascending' : sort === desc ? 'descending' : undefined);
  const tagLabel = $derived(tagFilter.length ? tr('lnkTagsFiltered', { tags: tagFilter.join(', ') }) : tr('tags'));

  // ---------- Selection ----------
  function toggleRow(l: CatalogLink, checked: boolean) {
    const next = new Set(selected);
    if (checked && next.size < MAX_SELECTION) next.add(l.selection_key);
    if (!checked) next.delete(l.selection_key);
    selected = next;
  }
  function togglePage() {
    const next = new Set(selected);
    if (allSelected) for (const l of pageRows) next.delete(l.selection_key);
    else for (const l of pageSelectable) if (next.size < MAX_SELECTION) next.add(l.selection_key);
    selected = next;
  }

  // ---------- Row actions ----------
  function patchItem(id: number, changes: Partial<CatalogLink>, key?: string) {
    items = items.map((l) => (l.id === id && (key === undefined || l.selection_key === key) ? { ...l, ...changes } : l));
    const l = items.find((x) => x.id === id);
    if (l) linkCache.set(id, l);
  }
  function inView(o: LinkOrganization) {
    if (view === 'archived') return o.archived;
    return !o.archived && (view !== 'favorites' || o.favorite);
  }

  async function toggleEnabled(l: CatalogLink) {
    if (updating !== null) return;
    updating = l.id;
    try {
      const updated = await app.api<Link>(`/links/${l.id}`, 'PATCH', { enabled: !l.enabled });
      patchItem(l.id, { ...updated, state: updated.enabled ? (l.state === 'disabled' ? 'active' : l.state) : 'disabled' });
      toast(tr('linkUpdated'), { body: `${l.host}/${l.slug}` });
      linksChanged();
    } catch (error) {
      toast(tr(problemMessage(error)), { tone: 'error' });
    } finally {
      updating = null;
    }
  }

  async function organize(l: CatalogLink, change: Partial<LinkOrganization>, message: keyof Messages) {
    if (updating !== null) return;
    updating = l.id;
    try {
      const updated = await app.api<LinkOrganization>(`/links/${l.id}/organization`, 'PATCH', { key: l.selection_key, ...change });
      patchItem(l.id, updated, l.selection_key);
      if (!inView(updated) && view !== 'attention') {
        items = items.filter((x) => x.id !== l.id);
        toggleRow(l, false);
        await tick();
        // Continue from the view switcher only when focus was in the removed row; focus that moved
        // elsewhere while the request ran stays there.
        if (!document.activeElement || document.activeElement === document.body) {
          viewGroup?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
        }
      }
      toast(tr(message), { body: `${l.host}/${l.slug}` });
      linksChanged();
    } catch (error) {
      const message = problemMessage(error);
      toast(tr(message), { tone: 'error' });
      if (message === 'linkOrganizationChanged') linksChanged();
    } finally {
      updating = null;
    }
  }

  async function removeOne(ev: SubmitEvent) {
    ev.preventDefault();
    if (!removing || removeBusy) return;
    const l = removing;
    removeBusy = true;
    removeError = null;
    try {
      await app.api<void>(`/links/${l.id}`, 'DELETE');
      items = items.filter((x) => x.id !== l.id);
      linkCache.delete(l.id);
      toggleRow(l, false);
      removing = null;
      toast(tr('linkDeleted'), { body: `${l.host}/${l.slug}` });
      if (app.counts.links != null && !l.archived) app.counts.links = Math.max(0, app.counts.links - 1);
      linksChanged();
      void loadTags();
      await tick();
      document.querySelector<HTMLElement>('main h1')?.focus();
    } catch (error) {
      removeError = problemMessage(error);
    } finally {
      removeBusy = false;
    }
  }

  function duplicate(l: CatalogLink) {
    createSeed.link = l;
    openCreate(l.target);
  }

  // ---------- Bulk actions ----------
  const identities = (list: CatalogLink[]) => list.map(({ id, selection_key }) => ({ id, key: selection_key }));

  async function bulk(change: Record<string, unknown>, list = selectedLinks): Promise<boolean> {
    if (bulkBusy || !list.length) return false;
    bulkBusy = true;
    bulkError = null;
    try {
      const result = await app.api<BulkLinkResult>('/links/bulk', 'POST', { links: identities(list), change });
      const keys = new Map(list.map((l) => [l.id, l.selection_key]));
      for (const u of result.items) patchItem(u.id, u, keys.get(u.id));
      const deleted = new Set(result.deleted_ids);
      items = items.filter((l) => !deleted.has(l.id) || l.selection_key !== keys.get(l.id));
      for (const id of deleted) linkCache.delete(id);
      selected = new Set();
      toast(tr(result.deleted_ids.length ? 'bulkLinksDeleted' : 'bulkLinksUpdated'), { body: nLinks(list.length) });
      linksChanged();
      void loadTags();
      return true;
    } catch (error) {
      bulkError = problemMessage(error);
      return false;
    } finally {
      bulkBusy = false;
    }
  }

  // Favorites and the archive are per-link organization; the bulk endpoint does not cover them.
  async function bulkOrganize(change: Partial<LinkOrganization>) {
    if (bulkBusy || !selectedLinks.length) return;
    bulkBusy = true;
    bulkError = null;
    const list = [...selectedLinks];
    let done = 0;
    try {
      for (const l of list) {
        const updated = await app.api<LinkOrganization>(`/links/${l.id}/organization`, 'PATCH', { key: l.selection_key, ...change });
        patchItem(l.id, updated, l.selection_key);
        done++;
      }
      selected = new Set();
      toast(tr('bulkLinksUpdated'), { body: nLinks(done) });
    } catch (error) {
      bulkError = problemMessage(error);
      if (done) toast(tr('lnkBulkPartial', { done, total: list.length }), { tone: 'info' });
    } finally {
      bulkBusy = false;
      if (done) linksChanged();
    }
  }

  async function bulkExport(format: 'json' | 'csv') {
    if (bulkBusy || !selectedLinks.length) return;
    bulkBusy = true;
    bulkError = null;
    try {
      const body = { links: identities(selectedLinks), format };
      const file = format === 'csv'
        ? await (await app.api<Response>('/links/bulk/export', 'POST', body, 'csv')).blob()
        : new Blob([JSON.stringify(await app.api<unknown>('/links/bulk/export', 'POST', body))], { type: 'application/json' });
      downloadFile(file, `shortcircuit-selected-links.${format}`);
    } catch (error) {
      bulkError = problemMessage(error);
    } finally {
      bulkBusy = false;
    }
  }

  async function applyTags() {
    if (!tagDialog || !bulkTags.length) return;
    if (await bulk({ action: tagDialog, tags: bulkTags })) {
      tagDialog = null;
      bulkTags = [];
    }
  }

  async function confirmBulkDelete(ev: SubmitEvent) {
    ev.preventDefault();
    if (!bulkDelete || bulkConfirm.trim() !== String(bulkDelete.length)) return;
    const list = bulkDelete;
    if (await bulk({ action: 'delete', confirmed: true }, list)) {
      bulkDelete = null;
      if (app.counts.links != null) app.counts.links = Math.max(0, app.counts.links - list.filter((l) => !l.archived).length);
      await tick();
      document.querySelector<HTMLElement>('main h1')?.focus();
    } else if (bulkError === 'bulkSelectionChanged') bulkDelete = null;
  }

  function reloadAfterChange() {
    bulkError = null;
    selected = new Set();
    linksChanged();
  }

  function exportShown() {
    const listed = items.map(({ selection_key: _k, favorite: _f, archived: _a, recorded_redirects: _r, last_redirect_date: _d, last_redirect_at: _t, ...l }) => {
      const { state: _s, starts_at: _st, expires_at: _e, max_visits: _m, visits_used: _u, protection: _p, rule_count: _c, visits_30d: _v, trend: _tr, monitor: _mo, preview: _pv, ...link } = l;
      return link;
    });
    downloadFile(new Blob([JSON.stringify(listed, null, 2) + '\n'], { type: 'application/json' }), 'shortcircuit-links.json');
  }

  // ---------- Display helpers ----------
  function stateOf(l: CatalogLink) {
    return linkState({ archived: l.archived, state: l.state ?? (l.enabled ? 'active' : 'disabled'), expires_at: l.expires_at ?? null });
  }
  function nLinks(n: number) {
    return n === 1 ? tr('lnkOneLink') : tr('lnkNLinks', { count: num(n) });
  }
  function opened(l: CatalogLink) {
    linkCache.set(l.id, l);
  }

  const description = $derived(
    isTags ? tr('lnkTagsDescription')
      : allCount != null ? `${nLinks(allCount)} ${hosts.length === 1 ? tr('lnkOnOneDomain') : tr('lnkOnDomains', { count: num(hosts.length) })}`
      : tr('linksDescription'),
  );
  const viewCount = (v: View) => (v === 'all' ? (allCount == null ? null : num(allCount)) : v === 'archived' ? (counts.archived == null ? null : num(counts.archived)) : v === 'attention' ? (counts.attention == null ? null : num(counts.attention)) : counts.favorites);
  const rangeText = $derived(
    items.length === 0 ? '' : tr(cursor ? 'lnkRangeMore' : 'lnkRange', { start: num(page * perPage + 1), end: num(Math.min(items.length, (page + 1) * perPage)), total: num(items.length) }),
  );
</script>

<div class="page">
  <PageHeader title={isTags ? tr('navTags') : tr('navLinks')} {description}>
    {#snippet actions()}
      <a class="btn" href={href('exchange')}><Icon name="upload" />{tr('lnkImport')}</a>
      {#if !isTags}
        <Menu label={tr('lnkExport')} text={tr('lnkExport')} variant="secondary">
          {#snippet children(close)}
            <button class="menu-item" role="menuitem" disabled={!items.length} onclick={() => { exportShown(); close(); }}><Icon name="file-down" />{tr('lnkExportShown')}<span class="meta">{num(items.length)}</span></button>
            <a class="menu-item" role="menuitem" href={href('exchange')} onclick={close}><Icon name="arrow-left-right" />{tr('lnkExportMore')}</a>
          {/snippet}
        </Menu>
      {/if}
    {/snippet}
  </PageHeader>

  <nav class="tabs" aria-label={tr('lnkSections')}>
    <a class="tab" href={href('links')} aria-current={!isTags ? 'page' : undefined}><Icon name="link" />{tr('lnkAllLinks')}</a>
    <a class="tab" href={href('tags')} aria-current={isTags ? 'page' : undefined}><Icon name="tag" />{tr('navTags')}<span class="count">{num(app.tags.length)}</span></a>
  </nav>

  {#if isTags}
    <Tags />
  {:else}
    <section class="panel links-panel" aria-label={tr('lnkListLabel')} aria-busy={loading}>
      <header class="panel-head list-head">
        {#if selected.size}
          <div class="toolbar bulk" role="toolbar" aria-label={tr('bulkActions')} aria-busy={bulkBusy}>
            <button class="btn" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('bulkClear')} use:tip={tr('bulkClear')} disabled={bulkBusy} onclick={() => { selected = new Set(); bulkError = null; }}><Icon name="x" /></button>
            <b class="num" role="status">{tr('lnkSelected', { count: num(selected.size) })}</b>
            <span class="sep"></span>
            <button class="btn" data-size="sm" disabled={bulkBusy} onclick={() => bulk({ action: 'set_enabled', enabled: true })}><Icon name="play" />{tr('enable')}</button>
            <button class="btn" data-size="sm" disabled={bulkBusy} onclick={() => bulk({ action: 'set_enabled', enabled: false })}><Icon name="circle-pause" />{tr('disable')}</button>
            {#if view !== 'archived'}
              <button class="btn" data-size="sm" disabled={bulkBusy} onclick={() => bulkOrganize({ favorite: true })}><Icon name="star" />{tr('favorite')}</button>
              <button class="btn" data-size="sm" disabled={bulkBusy} onclick={() => bulkOrganize({ archived: true })}><Icon name="archive" />{tr('lnkArchive')}</button>
            {:else}
              <button class="btn" data-size="sm" disabled={bulkBusy} onclick={() => bulkOrganize({ archived: false })}><Icon name="rotate-ccw" />{tr('lnkRestore')}</button>
            {/if}
            <Menu label={tr('tags')} text={tr('tags')} variant="secondary" size="sm" align="start">
              {#snippet children(close)}
                <button class="menu-item" role="menuitem" onclick={() => { close(); bulkTags = []; tagDialog = 'add_tags'; }}><Icon name="plus" />{tr('bulkAddTags')}</button>
                <button class="menu-item" role="menuitem" onclick={() => { close(); bulkTags = []; tagDialog = 'remove_tags'; }}><Icon name="x" />{tr('bulkRemoveTags')}</button>
              {/snippet}
            </Menu>
            <Menu label={tr('lnkExport')} text={tr('lnkExport')} variant="secondary" size="sm" align="start">
              {#snippet children(close)}
                <button class="menu-item" role="menuitem" onclick={() => { close(); void bulkExport('json'); }}><Icon name="file-down" />{tr('bulkExportJson')}</button>
                <button class="menu-item" role="menuitem" onclick={() => { close(); void bulkExport('csv'); }}><Icon name="file-down" />{tr('bulkExportCsv')}</button>
              {/snippet}
            </Menu>
            <span class="spacer"></span>
            <button class="btn" data-size="sm" data-variant="danger-quiet" disabled={bulkBusy} onclick={() => { bulkError = null; bulkConfirm = ''; bulkDelete = [...selectedLinks]; }}><Icon name="trash-2" />{tr('delete')}</button>
          </div>
        {:else}
          <div class="views" role="radiogroup" aria-label={tr('linkView')} bind:this={viewGroup}>
            {#each VIEWS as [v, label] (v)}
              {@const n = viewCount(v)}
              <button class="chip" role="radio" aria-checked={view === v} data-active={view === v || undefined} onclick={() => (view = v)}>
                {tr(label)}{#if n != null}<span class="count">{n}</span>{/if}
              </button>
            {/each}
          </div>
          <span class="spacer"></span>
          <div class="search list-search">
            <Icon name="search" />
            <input class="input" type="search" maxlength="4096" placeholder={tr('lnkFilterPlaceholder')} aria-label={tr('lnkFilterLabel')} bind:value={q} />
          </div>
        {/if}
      </header>

      {#if bulkError}
        <div class="list-notice">
          <div class="notice" data-tone="error" role="alert">
            <Icon name="circle-alert" /><div><p>{tr(bulkError)}</p></div>
            {#if bulkError === 'bulkSelectionChanged' || bulkError === 'linkOrganizationChanged'}<button class="btn" data-size="sm" onclick={reloadAfterChange}>{tr('bulkReload')}</button>{/if}
          </div>
        </div>
      {/if}

      <div class="filters" data-open={filtersOpen || undefined}>
        <button class="btn filters-toggle" data-size="sm" aria-expanded={filtersOpen} onclick={() => (filtersOpen = !filtersOpen)}>
          <Icon name="sliders-horizontal" />{tr('lnkFilters')}{#if filtersActive}<span class="dot active-dot" aria-label={tr('lnkFiltersActive')}></span>{/if}
        </button>
        <select class="select filter-control" data-size="sm" aria-label={tr('statusFilter')} bind:value={status} data-set={status || undefined}>
          <option value="">{tr('lnkAnyStatus')}</option>
          {#each STATES as [value, label] (value)}<option {value}>{tr(label)}</option>{/each}
        </select>
        <select class="select filter-control" data-size="sm" aria-label={tr('domainFilter')} bind:value={host} data-set={host || undefined}>
          <option value="">{tr('lnkAllDomains')}</option>
          {#each hosts as h (h)}<option value={h}>{h}</option>{/each}
        </select>
        <span class="filter-control">
          <Menu label={tr('tagFilter')} text={tagLabel} variant={tagFilter.length ? 'outline' : 'secondary'} size="sm" align="start" width={240}>
            {#snippet children()}
              {#if tagFilter.length > 1}
                <div class="menu-label">{tr('tagMatch')}</div>
                <button class="menu-item" role="menuitemradio" aria-checked={tagMatch === 'all'} onclick={() => (tagMatch = 'all')}>{tr('matchAllTags')}</button>
                <button class="menu-item" role="menuitemradio" aria-checked={tagMatch === 'any'} onclick={() => (tagMatch = 'any')}>{tr('matchAnyTag')}</button>
                <div class="menu-sep"></div>
              {/if}
              <div class="menu-label">{tagFilter.length > 1 && tagMatch === 'any' ? tr('lnkTagsAny') : tr('lnkTagsAll')}</div>
              {#each app.tags as t (t.name)}
                <button class="menu-item" role="menuitemcheckbox" aria-checked={tagFilter.includes(t.name)} disabled={!tagFilter.includes(t.name) && tagFilter.length >= MAX_TAG_FILTERS} onclick={() => toggleTag(t.name)}>
                  <i class="sw" style:background={tagColor(t.name)}></i>{t.name}<span class="meta">{num(t.link_count)}</span>
                </button>
              {:else}
                <p class="menu-label">{tr('noTags')}</p>
              {/each}
            {/snippet}
          </Menu>
        </span>
        <span class="filter-control" hidden={view === 'attention'}>
          <Menu label={tr('sortLinks')} text={tr(SORT_LABELS[sort])} icon="arrow-up-down" variant="secondary" size="sm" align="start" width={220}>
            {#snippet children(close)}
              <div class="menu-label">{tr('sortLinks')}</div>
              {#each linkSorts as s (s)}
                <button class="menu-item" role="menuitemradio" aria-checked={sort === s} onclick={() => { sort = s; close(); }}>{tr(SORT_LABELS[s])}</button>
              {/each}
            {/snippet}
          </Menu>
        </span>
        {#if filtersActive}
          <button class="btn" data-variant="ghost" data-size="sm" onclick={clearFilters}><Icon name="x" />{tr('resetFilters')}</button>
        {/if}
        <span class="spacer"></span>
        {#if filtersActive && initialized}<span class="muted result-count num" aria-live="polite">{tr(cursor ? 'lnkMatchesMore' : 'lnkMatches', { count: num(items.length) })}</span>{/if}
        <div class="seg" data-size="sm" role="radiogroup" aria-label={tr('lnkDensity')}>
          <button role="radio" aria-checked={density === 'comfortable'} aria-label={tr('lnkComfortable')} use:tip={tr('lnkComfortable')} onclick={() => (density = 'comfortable')}><Icon name="menu" /></button>
          <button role="radio" aria-checked={density === 'compact'} aria-label={tr('lnkCompact')} use:tip={tr('lnkCompact')} onclick={() => (density = 'compact')}><Icon name="list-filter" /></button>
        </div>
        <Menu label={tr('lnkColumns')} icon="columns-3" size="sm" width={200}>
          {#snippet children()}
            <div class="menu-label">{tr('lnkShowColumns')}</div>
            {#each COLUMNS as [key, label] (key)}
              <button class="menu-item" role="menuitemcheckbox" aria-checked={show(key)} onclick={() => (hidden = show(key) ? [...hidden, key] : hidden.filter((h) => h !== key))}>{tr(label)}</button>
            {/each}
          {/snippet}
        </Menu>
      </div>

      {#if loadError && !items.length}
        <div class="list-notice">
          <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(loadError)}</p></div><button class="btn" data-size="sm" onclick={() => retry?.()}>{tr('retry')}</button></div>
        </div>
      {:else if !initialized}
        <p class="muted list-loading" role="status">{tr('loading')}</p>
      {:else if items.length === 0}
        {#if filtersActive}
          <Empty icon="search" title={tr('lnkNoMatchTitle')}>
            {tr('lnkNoMatchHint')}
            {#snippet actions()}<button class="btn" onclick={clearFilters}>{tr('resetFilters')}</button>{/snippet}
          </Empty>
        {:else if view === 'favorites'}
          <Empty icon="star" title={tr('noFavoriteLinks')}>{tr('lnkNoFavoritesHint')}</Empty>
        {:else if view === 'archived'}
          <Empty icon="archive" title={tr('noArchivedLinks')}>{tr('lnkNoArchivedHint')}</Empty>
        {:else if view === 'attention'}
          <Empty icon="circle-check" title={tr('lnkNoAttentionTitle')}>{tr('lnkNoAttentionHint')}</Empty>
        {:else}
          <Empty icon="link" title={tr('lnkNoLinksTitle')}>
            {tr('lnkNoLinksHint')}
            {#snippet actions()}<button class="btn" onclick={() => openCreate()}><Icon name="plus" />{tr('newLink')}</button><a class="btn" href={href('exchange')}>{tr('lnkImport')}</a>{/snippet}
          </Empty>
        {/if}
      {:else}
        <div class="table-wrap list-wrap">
          <table class="table links-table" data-density={density}>
            <thead>
              <tr>
                <th class="c-check"><input class="checkbox" type="checkbox" aria-label={tr('lnkSelectPage')} checked={allSelected} indeterminate={someSelected} disabled={bulkBusy} onchange={togglePage} /></th>
                <th aria-sort={ariaSort('title_asc', 'title_desc')}><button class="th-sort" onclick={() => sortBy('title_asc', 'title_desc')}>{tr('lnkColLink')}<Icon name="arrow-up-down" /></button></th>
                {#if show('dest')}<th class="c-dest" aria-sort={ariaSort('target_asc', 'target_desc')}><button class="th-sort" onclick={() => sortBy('target_asc', 'target_desc')}>{tr('lnkColDestination')}<Icon name="arrow-up-down" /></button></th>{/if}
                {#if show('tags')}<th class="c-tags">{tr('tags')}</th>{/if}
                {#if show('visits')}<th class="r c-visits" aria-sort={ariaSort('visits_asc', 'visits_desc')}><button class="th-sort" onclick={() => sortBy('visits_desc', 'visits_asc')}>{tr('lnkColVisits')}<Icon name="arrow-up-down" /></button></th>{/if}
                {#if show('trend')}<th class="c-trend"><span class="visually-hidden">{tr('lnkColTrend')}</span></th>{/if}
                {#if show('status')}<th class="c-status">{tr('status')}</th>{/if}
                {#if show('health')}<th class="c-health">{tr('navHealth')}</th>{/if}
                {#if show('last')}<th class="r c-last" aria-sort={ariaSort('last_visit_asc', 'last_visit_desc')}><button class="th-sort" onclick={() => sortBy('last_visit_desc', 'last_visit_asc')}>{tr('lnkColLast')}<Icon name="arrow-up-down" /></button></th>{/if}
                <th class="c-actions"><span class="visually-hidden">{tr('lnkActions')}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each pageRows as l (l.id)}
                {@const d = splitUrl(l.target)}
                {@const name = l.title || l.slug}
                {@const isSelected = selected.has(l.selection_key)}
                <tr aria-selected={isSelected}>
                  <td class="c-check"><input class="checkbox" type="checkbox" aria-label={tr('lnkSelectLink', { name: `${l.host}/${l.slug}` })} checked={isSelected} disabled={bulkBusy || (!isSelected && selected.size >= MAX_SELECTION)} onchange={(e) => toggleRow(l, e.currentTarget.checked)} /></td>
                  <td class="c-link">
                    <div class="link-row">
                    {#if app.previews}<LinkPreview id={l.id} preview={l.preview} />{/if}
                    <div class="link-cell">
                      <a class="link-title" href={href('links', l.id)} onclick={() => opened(l)}>
                        {#if l.favorite}<Icon name="star" label={tr('favorite')} />{/if}
                        {#if l.protection}<Icon name="lock" label={l.protection === 'password' ? tr('lnkProtectedPassword') : tr('oneTimeLink')} />{/if}
                        <span class="truncate">{name}</span>
                      </a>
                      <span class="link-url"><ShortUrl host={l.host} slug={l.slug} /><CopyButton value={shortUrl(l)} label={tr('copyLabel', { value: `${l.host}/${l.slug}` })} /></span>
                      <span class="dest mobile-dest"><b>{d.host}</b>{d.rest}</span>
                      {#if l.description}<span class="link-desc truncate">{l.description}</span>{/if}
                    </div>
                    </div>
                  </td>
                  {#if show('dest')}
                    <td class="c-dest">
                      <span class="dest" title={l.target}><b>{d.host}</b>{d.rest}</span>
                      {#if l.rule_count}<span class="rules">{tr('lnkRuleCount', { count: l.rule_count })}</span>{/if}
                    </td>
                  {/if}
                  {#if show('tags')}
                    <td class="c-tags">
                      <div class="tags">
                        {#each l.tags.slice(0, 2) as t (t)}<TagChip name={t} onclick={() => { if (!tagFilter.includes(t)) toggleTag(t); }} />{/each}
                        {#if l.tags.length > 2}<span class="more" use:tip={l.tags.slice(2).join(', ')} aria-label={l.tags.slice(2).join(', ')} role="img">+{l.tags.length - 2}</span>{/if}
                      </div>
                    </td>
                  {/if}
                  {#if show('visits')}
                    <td class="r num c-visits">
                      {#if l.visits_30d === null}<span class="muted" use:tip={tr('lnkAnalyticsOffTip')}>{tr('lnkNotRecorded')}</span>
                      {:else if l.visits_30d === undefined}<span class="muted">—</span>
                      {:else}<b>{num(l.visits_30d)}</b><span class="visits-unit">{tr('lnkVisitsUnit')}</span>{/if}
                    </td>
                  {/if}
                  {#if show('trend')}<td class="c-trend"><Sparkline values={l.trend ?? []} width={64} /></td>{/if}
                  {#if show('status')}<td class="c-status"><Badge state={stateOf(l)} quiet /></td>{/if}
                  {#if show('health')}<td class="c-health"><HealthIcon health={health(l.monitor?.status)} /></td>{/if}
                  {#if show('last')}
                    <td class="r muted c-last">
                      {#if l.last_redirect_at != null}<time datetime={new Date(l.last_redirect_at * 1000).toISOString()}>{ago(l.last_redirect_at)}</time>
                      {:else if l.last_redirect_date}<time datetime={l.last_redirect_date} use:tip={tr('lastUsedDayHint')}>{day(l.last_redirect_date)}</time>
                      {:else}{tr('never')}{/if}
                    </td>
                  {/if}
                  <td class="c-actions">
                    <div class="row-actions">
                      <button class="btn reveal" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('lnkQrFor', { url: l.slug })} use:tip={tr('qrCode')} onclick={() => { opened(l); overlays.qrId = l.id; }}><Icon name="qr-code" /></button>
                      <button class="btn reveal" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('lnkEditName', { name: l.slug })} use:tip={tr('editLink')} disabled={bulkBusy} onclick={() => { opened(l); overlays.editId = l.id; }}><Icon name="pencil" /></button>
                      <Menu label={tr('moreActionsFor', { name: l.slug })} icon="ellipsis" size="sm">
                        {#snippet children(close)}
                          <a class="menu-item" role="menuitem" href={href('links', l.id)} onclick={() => { opened(l); close(); }}><Icon name="arrow-right" />{tr('lnkOpenDetails')}</a>
                          <button class="menu-item" role="menuitem" onclick={() => { close(); opened(l); overlays.editId = l.id; }}><Icon name="pencil" />{tr('editLink')}</button>
                          <button class="menu-item" role="menuitem" onclick={() => { close(); opened(l); overlays.qrId = l.id; }}><Icon name="qr-code" />{tr('qrCode')}</button>
                          <a class="menu-item" role="menuitem" href={shortUrl(l)} target="_blank" rel="noreferrer noopener" onclick={close}><Icon name="external-link" />{tr('lnkOpenShort')}</a>
                          <button class="menu-item" role="menuitem" disabled={!app.domains?.hosts.length} onclick={() => { close(); duplicate(l); }}><Icon name="copy-plus" />{tr('duplicateLink')}</button>
                          <button class="menu-item" role="menuitem" disabled={updating !== null} onclick={() => { close(); void organize(l, { favorite: !l.favorite }, l.favorite ? 'favoriteRemoved' : 'favoriteAdded'); }}><Icon name="star" />{l.favorite ? tr('lnkUnfavorite') : tr('lnkFavorite')}</button>
                          <button class="menu-item" role="menuitem" disabled={updating !== null} onclick={() => { close(); void toggleEnabled(l); }}><Icon name={l.enabled ? 'circle-pause' : 'play'} />{l.enabled ? tr('disable') : tr('enable')}</button>
                          <a class="menu-item" role="menuitem" href={href('links', l.id, { tab: 'history' })} onclick={() => { opened(l); close(); }}><Icon name="history" />{tr('linkHistory')}</a>
                          <div class="menu-sep"></div>
                          <button class="menu-item" role="menuitem" disabled={updating !== null} onclick={() => { close(); void organize(l, { archived: !l.archived }, l.archived ? 'linkUnarchived' : 'linkArchived'); }}><Icon name={l.archived ? 'rotate-ccw' : 'archive'} />{l.archived ? tr('unarchiveLink') : tr('archiveLink')}</button>
                          <button class="menu-item" role="menuitem" data-tone="danger" onclick={() => { close(); removeError = null; removing = l; }}><Icon name="trash-2" />{tr('deleteLink')}</button>
                        {/snippet}
                      </Menu>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if loadError}
          <div class="list-notice">
            <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(loadError)}</p></div><button class="btn" data-size="sm" onclick={() => retry?.()}>{tr('retry')}</button></div>
          </div>
        {/if}
        <footer class="pager">
          <span class="num" aria-live="polite">{loading ? tr('loading') : rangeText}</span>
          <span class="spacer"></span>
          <label class="pp">{tr('lnkRows')}
            <select class="select" data-size="sm" bind:value={perPage} onchange={() => (page = 0)}>
              <option value={10}>10</option><option value={25}>25</option><option value={50}>50</option>
            </select>
          </label>
          <button class="btn" data-size="sm" data-icon-only aria-label={tr('lnkPrevPage')} use:tip={tr('lnkPrevPage')} disabled={page === 0} onclick={() => page--}><Icon name="chevron-left" /></button>
          <button class="btn" data-size="sm" data-icon-only aria-label={tr('lnkNextPage')} use:tip={tr('lnkNextPage')} disabled={loading || ((page + 1) * perPage >= items.length && !cursor)} onclick={nextPage}><Icon name="chevron-right" /></button>
        </footer>
      {/if}
    </section>
  {/if}
</div>

<Dialog open={removing !== null} onclose={() => (removing = null)} title={tr('lnkDeleteOneTitle')} width={440} busy={removeBusy}>
  <form id="delete-one" onsubmit={removeOne}>
    <p class="secondary confirm-text"><code>{removing?.host}/{removing?.slug}</code> {tr('lnkDeleteOneHint')}</p>
    {#if removeError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(removeError)}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={removeBusy} onclick={() => (removing = null)}>{tr('cancel')}</button>
    <button type="submit" form="delete-one" class="btn" data-variant="danger" disabled={removeBusy} aria-busy={removeBusy}>{#if removeBusy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="trash-2" /><span>{tr('deleteLink')}</span></button>
  {/snippet}
</Dialog>

<Dialog open={bulkDelete !== null} onclose={() => (bulkDelete = null)} title={tr('bulkDeleteTitle')} width={480} busy={bulkBusy}>
  <form id="delete-many" class="stack" onsubmit={confirmBulkDelete}>
    <p class="secondary confirm-text">{tr('bulkDeleteHint')} {tr('lnkArchiveInstead')}</p>
    <ul class="delete-list">
      {#each bulkDelete ?? [] as l (l.selection_key)}<li class="mono">{l.host}/{l.slug}</li>{/each}
    </ul>
    <div class="field">
      <label class="field-label" for="bulk-confirm">{tr('lnkTypeToConfirm', { count: bulkDelete?.length ?? 0 })}</label>
      <input id="bulk-confirm" class="input narrow" inputmode="numeric" autocomplete="off" bind:value={bulkConfirm} />
    </div>
    {#if bulkError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(bulkError)}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={bulkBusy} onclick={() => (bulkDelete = null)}>{tr('cancel')}</button>
    <button type="submit" form="delete-many" class="btn" data-variant="danger" disabled={bulkBusy || bulkConfirm.trim() !== String(bulkDelete?.length ?? '')} aria-busy={bulkBusy}>
      {#if bulkBusy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="trash-2" /><span>{tr('bulkDelete')}</span>
    </button>
  {/snippet}
</Dialog>

<Dialog open={tagDialog !== null} onclose={() => (tagDialog = null)} title={tagDialog === 'remove_tags' ? tr('bulkRemoveTags') : tr('bulkAddTags')} description={nLinks(selectedLinks.length)} width={460} busy={bulkBusy}>
  <form id="bulk-tags" class="stack" onsubmit={(e) => { e.preventDefault(); void applyTags(); }}>
    <div class="field">
      <label class="field-label" for="bulk-tag-input">{tr('tags')}</label>
      <TagInput id="bulk-tag-input" bind:value={bulkTags} />
    </div>
    {#if bulkError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(bulkError)}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={bulkBusy} onclick={() => (tagDialog = null)}>{tr('cancel')}</button>
    <button type="submit" form="bulk-tags" class="btn" data-variant="primary" disabled={bulkBusy || !bulkTags.length} aria-busy={bulkBusy}>
      {#if bulkBusy}<span class="spinner" aria-hidden="true"></span>{/if}<span>{tr('bulkApply')}</span>
    </button>
  {/snippet}
</Dialog>

<style>
  .links-panel {
    container: links / inline-size;
  }
  .list-head {
    flex-wrap: wrap;
    padding-left: var(--space-4);
  }
  .views {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .list-search {
    width: min(340px, 100%);
  }
  .bulk {
    flex: 1;
    flex-wrap: wrap;
  }
  .bulk b {
    font-size: var(--text-sm);
  }
  .bulk .sep {
    width: 1px;
    height: 20px;
    margin: 0 4px;
    background: var(--border);
  }
  .list-notice {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border-subtle);
  }
  .list-loading {
    padding: var(--space-6) var(--space-4);
  }
  .filters {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    padding: 10px var(--space-4);
    border-bottom: 1px solid var(--border-subtle);
  }
  .filters .select {
    width: auto;
    min-width: 0;
    border-color: var(--border);
    background-color: transparent;
    color: var(--text-secondary);
    font-weight: var(--weight-medium);
  }
  .filters .select[data-set] {
    border-color: var(--accent-soft-border);
    background-color: var(--accent-soft);
    color: var(--text);
  }
  .filters .spacer {
    flex: 1;
  }
  .filters-toggle {
    display: none;
  }
  .active-dot {
    color: var(--accent);
  }
  .result-count {
    font-size: var(--text-xs);
    margin-right: 4px;
  }
  .sw {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-2xs);
  }
  .c-link {
    min-width: 240px;
  }
  /* The optional preview thumbnail sits before the link text, in the table and in cards. */
  .link-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .link-cell {
    display: grid;
    gap: 2px;
    min-width: 0;
    max-width: 340px;
  }
  .link-desc {
    color: var(--text-muted);
    font-size: var(--text-xs);
    line-height: var(--leading-xs);
  }
  .links-table[data-density='compact'] .link-desc {
    display: none;
  }
  .link-title {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text);
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    text-decoration: none;
  }
  .link-title:hover {
    color: var(--accent);
  }
  .link-title :global(svg) {
    flex: none;
    width: 13px;
    height: 13px;
    color: var(--text-muted);
  }
  .link-url {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    height: 22px;
  }
  .link-url :global(.short-url) {
    font-size: var(--text-xs);
  }
  .link-url :global(.btn) {
    --btn-h: 22px;
    --btn-icon: 13px;
    opacity: 0;
  }
  tr:hover .link-url :global(.btn),
  .link-url :global(.btn:focus-visible),
  .link-url :global(.btn[data-done]) {
    opacity: 1;
  }
  .links-table[data-density='compact'] .link-cell {
    grid-template-columns: minmax(0, auto) minmax(0, 1fr);
    align-items: center;
    column-gap: 10px;
    max-width: 520px;
  }
  .links-table[data-density='compact'] .link-title {
    font-size: var(--text-sm);
  }
  .c-dest .dest {
    max-width: 260px;
    font-size: var(--text-sm);
    line-height: var(--leading-sm);
  }
  .rules {
    display: block;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .links-table[data-density='compact'] .rules {
    display: none;
  }
  .tags {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .more {
    color: var(--text-muted);
    font-size: var(--text-xs);
    padding: 0 4px;
  }
  .c-visits b {
    font-weight: var(--weight-medium);
  }
  .visits-unit {
    display: none;
    margin-left: 4px;
    color: var(--text-muted);
  }
  .c-trend {
    width: 80px;
    padding-left: 4px !important;
  }
  .c-health {
    width: 56px;
    padding-inline: 4px !important;
    text-align: center;
  }
  .c-last {
    white-space: nowrap;
    font-size: var(--text-xs);
  }
  .mobile-dest {
    display: none;
  }
  .pp {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-xs);
  }
  .pp .select {
    width: 68px;
  }
  .confirm-text {
    font-size: var(--text-sm);
  }
  .delete-list {
    max-height: min(40dvh, 18rem);
    overflow: auto;
    padding-left: var(--space-5);
    list-style: disc;
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  .narrow {
    max-width: 160px;
  }

  @container links (max-width: 1300px) {
    .c-last {
      display: none;
    }
  }
  @container links (max-width: 1180px) {
    .c-trend {
      display: none;
    }
  }
  @container links (max-width: 1000px) {
    .c-tags {
      display: none;
    }
  }
  @container links (max-width: 820px) {
    .c-dest {
      display: none;
    }
    .mobile-dest {
      display: block;
      max-width: 420px;
    }
  }
  /* Narrow containers: each row becomes a card. */
  @container links (max-width: 640px) {
    .list-wrap {
      overflow: visible;
    }
    .links-table thead {
      display: none;
    }
    .links-table,
    .links-table tbody {
      display: block;
    }
    .links-table tr {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto auto;
      grid-template-areas: 'check link link actions' 'check visits status health';
      gap: 6px 10px;
      padding: 12px var(--space-4);
      border-bottom: 1px solid var(--border-subtle);
    }
    .links-table td {
      display: block;
      height: auto;
      padding: 0 !important;
      border: 0;
    }
    .links-table .c-dest,
    .links-table .c-tags,
    .links-table .c-trend,
    .links-table .c-last {
      display: none;
    }
    .links-table .c-check {
      grid-area: check;
      padding-top: 3px !important;
    }
    .links-table .c-link {
      grid-area: link;
      min-width: 0;
    }
    .links-table .c-actions {
      grid-area: actions;
    }
    .links-table .c-visits,
    .links-table .c-status,
    .links-table .c-health {
      display: inline-flex;
      align-items: center;
      gap: 6px;
    }
    .links-table .c-visits {
      grid-area: visits;
      gap: 0;
      font-size: var(--text-xs);
    }
    .visits-unit {
      display: inline;
    }
    .links-table .c-status {
      grid-area: status;
    }
    .links-table .c-health {
      grid-area: health;
      justify-self: end;
      width: auto;
    }
    .links-table .row-actions .reveal {
      display: none;
    }
    /* Touch targets on phones are at least 24 px (WCAG 2.5.8). */
    .link-title {
      min-height: 24px;
    }
    .link-url {
      height: 24px;
    }
    .link-url :global(.btn) {
      --btn-h: 24px;
      opacity: 1;
    }
    .list-search {
      width: 100%;
    }
    .filters-toggle {
      display: inline-flex;
    }
    .filters:not([data-open]) .filter-control {
      display: none;
    }
    .views {
      mask-image: linear-gradient(to right, black calc(100% - 32px), transparent);
      flex-wrap: nowrap;
      overflow-x: auto;
      scrollbar-width: none;
      margin-right: calc(-1 * var(--space-4));
      padding-right: var(--space-4);
    }
  }
</style>
