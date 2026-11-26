<script lang="ts">
  import { onMount } from 'svelte';
  import { linkName, linkSorts, shortUrl } from './api';
  import type { Api, BulkLinkResult, CatalogLink, Domains, Link, LinkCursor, LinkMetadata, LinkOrganization, LinkProtectionChange, LinkSearch, LinkSearchPage, LinkSort, LinkView, Tag, TagMatch } from './api';
  import BulkLinks from './BulkLinks.svelte';
  import Dialog from './Dialog.svelte';
  import { downloadFile, formatDate, passwordAllowed, problemMessage } from './format';
  import DuplicateLookup from './DuplicateLookup.svelte';
  import UrlCleaner from './UrlCleaner.svelte';
  import LinkQr from './LinkQr.svelte';
  import LinkHistory from './LinkHistory.svelte';
  import LinkProtectionEditor from './LinkProtectionEditor.svelte';
  import PolicyEditor from './PolicyEditor.svelte';
  import ProtectionFields from './ProtectionFields.svelte';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  let { t, language, api, domains }: { t: Messages; language: Language; api: Api; domains: Domains } = $props();

  const maxSelection = 50;
  let items = $state<CatalogLink[]>([]);
  let selected = $state.raw(new Set<string>());
  let selectShownInput = $state<HTMLInputElement>();
  let bulkBusy = $state(false);
  let cursor = $state.raw<LinkCursor | null>(null);
  let loading = $state(false);
  let initialized = $state(false);
  let loadError = $state<keyof Messages | null>(null);
  let search = $state('');
  let tagsFilter = $state.raw<string[]>([]);
  let tagMatch = $state<TagMatch>('all');
  let view = $state<LinkView>('unarchived');
  let viewControl = $state<HTMLSelectElement>();
  let domain = $state('');
  let status = $state('');
  let sort = $state<LinkSort>('newest');
  let catalogTags = $state.raw<string[]>([]);
  let revision = $state(0);
  let notice = $state<keyof Messages | null>(null);
  let actionError = $state<keyof Messages | null>(null);
  let updating = $state<number | null>(null);

  let formOpen = $state(false);
  let editing = $state<Link | null>(null);
  let qrLink = $state<Link | null>(null);
  let historyLink = $state<Link | null>(null);
  let policyLink = $state<Link | null>(null);
  let protectionLink = $state<Link | null>(null);
  let removing = $state<Link | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let saving = $state(false);
  let target = $state('');
  let title = $state('');
  let slug = $state('');
  let host = $state('');
  let tags = $state('');
  let analyticsEnabled = $state(true);
  let oneTime = $state(false);
  let linkPassword = $state('');

  const availableTags = $derived([...new Set([
    ...catalogTags, ...items.flatMap((link) => link.tags),
  ])].filter((tag) => !tagsFilter.includes(tag)).sort((a, b) => a.localeCompare(b, language)));
  const availableDomains = $derived(Object.keys(domains.origins).sort());
  const selectedLinks = $derived(items.filter((item) => selected.has(item.selection_key)));
  const selectable = $derived(items.slice(0, maxSelection));
  const allShownSelected = $derived(selectable.length > 0 && selectable.every((item) => selected.has(item.selection_key)));
  const limit = $derived(new Intl.NumberFormat(language));
  const tagRequests = new AbortController();
  let tagGeneration = 0;
  let currentQuery: LinkSearch = { sort: 'newest', view: 'unarchived' };
  let queryKey = '';
  let loadedPages = 0;
  let pageRequest: AbortController | null = null;
  let retryPage: (() => void) | null = null;
  const sortLabels = {
    newest: 'sortNewest', oldest: 'sortOldest',
    title_asc: 'sortTitleAscending', title_desc: 'sortTitleDescending',
    slug_asc: 'sortSlugAscending', slug_desc: 'sortSlugDescending',
    host_asc: 'sortHostAscending', host_desc: 'sortHostDescending',
    target_asc: 'sortTargetAscending', target_desc: 'sortTargetDescending',
    enabled_first: 'sortEnabledFirst', disabled_first: 'sortDisabledFirst',
  } as const satisfies Record<LinkSort, keyof Messages>;

  onMount(() => {
    void loadTags();
    return () => tagRequests.abort();
  });

  $effect(() => {
    if (selectShownInput) selectShownInput.indeterminate = selectedLinks.length > 0 && !allShownSelected;
  });

  $effect(() => {
    void revision;
    const criteria: LinkSearch = {
      query: search.trim() || undefined,
      host: domain || undefined,
      tags: tagsFilter.length ? tagsFilter : undefined,
      tag_match: tagsFilter.length > 1 ? tagMatch : undefined,
      enabled: status ? status === 'enabled' : undefined,
      view,
      sort,
    };
    const key = JSON.stringify(criteria);
    const retainedDepth = key === queryKey ? loadedPages : 0;
    const pending = new AbortController();
    pageRequest = pending;
    currentQuery = criteria;
    queryKey = key;
    if (retainedDepth === 0) {
      items = [];
      selected = new Set();
      cursor = null;
      initialized = false;
      loadedPages = 0;
    }
    loading = true;
    loadError = null;
    const timer = setTimeout(
      () => { void loadPages(criteria, null, pending.signal, Math.max(1, retainedDepth)); },
      retainedDepth ? 0 : 150,
    );
    return () => {
      clearTimeout(timer);
      pending.abort();
    };
  });

  async function loadTags() {
    const generation = ++tagGeneration;
    try {
      const page = await api<{ items: Tag[] }>('/catalog/tags', 'GET', undefined, 'json', tagRequests.signal);
      if (!tagRequests.signal.aborted && generation === tagGeneration) {
        catalogTags = page.items.map(({ name }) => name);
        if (tagsFilter.some((tag) => !catalogTags.includes(tag))) {
          tagsFilter = tagsFilter.filter((tag) => catalogTags.includes(tag));
        }
      }
    } catch {
      // Tags from the fetched links remain usable when the catalog is unavailable.
    }
  }

  async function loadPages(criteria: LinkSearch, after: LinkCursor | null, signal: AbortSignal, pages = 1) {
    loading = true;
    loadError = null;
    retryPage = null;
    try {
      const collected = after ? [...items] : [];
      const seen = new Set(collected.map((link) => link.id));
      let next = after;
      let depth = after ? loadedPages : 0;
      for (let index = 0; index < pages; index += 1) {
        const page = await api<LinkSearchPage>('/links/search', 'POST', { ...criteria, after: next ?? undefined }, 'json', signal);
        if (signal.aborted) return;
        for (const link of page.items) {
          if (!seen.has(link.id)) collected.push(link);
          seen.add(link.id);
        }
        depth += 1;
        next = page.next_cursor;
        if (!next) break;
      }
      items = collected;
      selected = new Set(collected.map((item) => item.selection_key).filter((key) => selected.has(key)));
      cursor = next;
      loadedPages = depth;
      initialized = true;
    } catch (error) {
      if (!signal.aborted) {
        loadError = problemMessage(error);
        retryPage = () => { void loadPages(criteria, after, signal, pages); };
      }
    } finally {
      if (!signal.aborted) loading = false;
    }
  }

  function loadMore() {
    if (!loading && pageRequest && cursor) void loadPages(currentQuery, cursor, pageRequest.signal);
  }

  function refreshCatalog(refreshTags = true) {
    pageRequest?.abort();
    revision += 1;
    if (refreshTags) void loadTags();
  }

  function updateSelection(key: string, checked: boolean) {
    const next = new Set(selected);
    if (checked && next.size < maxSelection) next.add(key);
    if (!checked) next.delete(key);
    selected = next;
  }

  function applyBulk(result: BulkLinkResult, selection: CatalogLink[]) {
    const keys = new Map(selection.map((item) => [item.id, item.selection_key]));
    for (const updated of result.items) {
      if (items.some((item) => item.id === updated.id && item.selection_key === keys.get(item.id))) {
        patchListedLink(updated.id, updated);
      }
    }
    const deleted = new Set(result.deleted_ids);
    items = items.filter((item) => !deleted.has(item.id) || item.selection_key !== keys.get(item.id));
    selected = new Set();
    notice = result.deleted_ids.length ? 'bulkLinksDeleted' : 'bulkLinksUpdated';
    refreshCatalog();
  }

  function inCurrentView(link: LinkOrganization) {
    if (view === 'archived') return link.archived;
    return !link.archived && (view === 'unarchived' || link.favorite);
  }

  function patchListedLink(id: number, changes: Partial<LinkMetadata & LinkOrganization>, key?: string) {
    items = items.flatMap((link) => {
      if (link.id !== id || (key !== undefined && link.selection_key !== key)) return [link];
      const updated = { ...link, ...changes };
      const text = search.trim().toLowerCase();
      const matchesTags = tagsFilter.length === 0 || (tagMatch === 'all'
        ? tagsFilter.every((tag) => updated.tags.includes(tag))
        : tagsFilter.some((tag) => updated.tags.includes(tag)));
      const matching = inCurrentView(updated) && matchesTags
        && (!domain || domain === updated.host)
        && (!status || updated.enabled === (status === 'enabled'))
        && (!text || [updated.title, updated.slug, updated.host, updated.target,
          `${updated.host}/${updated.slug}`, shortUrl(updated, domains), ...updated.tags]
          .some((value) => value.toLowerCase().includes(text)));
      return matching ? [updated] : [];
    });
  }

  function restoreMetadata(id: number, metadata: LinkMetadata) {
    patchListedLink(id, metadata);
    refreshCatalog();
  }

  function clearFilters() {
    search = '';
    tagsFilter = [];
    tagMatch = 'all';
    domain = '';
    status = '';
  }

  function toggleTag(tag: string) {
    if (tagsFilter.includes(tag)) tagsFilter = tagsFilter.filter((value) => value !== tag);
    else if (tagsFilter.length < 20) tagsFilter = [...tagsFilter, tag];
  }

  function openForm(link: Link | null = null) {
    editing = link;
    target = link?.target ?? '';
    title = link?.title ?? '';
    slug = link?.slug ?? '';
    host = link?.host ?? domains.hosts[0] ?? '';
    tags = link?.tags.join(', ') ?? '';
    analyticsEnabled = link?.analytics_enabled ?? true;
    oneTime = false;
    linkPassword = '';
    formError = null;
    formOpen = true;
  }

  function closeForm() {
    linkPassword = '';
    formOpen = false;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    formError = null;
    const value = target.trim();
    try {
      const url = new URL(value);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error();
    } catch {
      formError = 'invalidTarget';
      return;
    }
    if (!editing && linkPassword && !passwordAllowed(linkPassword)) {
      formError = 'invalidPassword';
      return;
    }

    const fields = {
      target: value,
      title: title.trim(),
      tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean),
    };
    saving = true;
    notice = null;
    actionError = null;
    try {
      if (editing) {
        const changes = {
          ...(fields.target !== editing.target ? { target: fields.target } : {}),
          ...(analyticsEnabled !== editing.analytics_enabled ? { analytics_enabled: analyticsEnabled } : {}),
          ...(fields.title !== editing.title ? { title: fields.title } : {}),
          ...(fields.tags.join(', ') !== editing.tags.join(', ') ? { tags: fields.tags } : {}),
        };
        if (!Object.keys(changes).length) {
          closeForm();
          return;
        }
        const updated = await api<Link>(`/links/${editing.id}`, 'PATCH', changes);
        patchListedLink(updated.id, updated);
        notice = 'linkUpdated';
      } else {
        const protection: Omit<LinkProtectionChange, 'remove_password'> | undefined = oneTime || linkPassword ? {
          one_time: oneTime,
          ...(linkPassword ? { password: linkPassword } : {}),
        } : undefined;
        await api<Link>('/links', 'POST', {
          ...fields,
          ...(host ? { host } : {}),
          ...(slug.trim() ? { slug: slug.trim() } : {}),
          ...(protection ? { protection } : {}),
        });
        clearFilters();
        view = 'unarchived';
        notice = 'linkCreated';
      }
      refreshCatalog();
      closeForm();
    } catch (error) {
      formError = problemMessage(error);
    } finally {
      saving = false;
    }
  }

  async function toggle(link: Link) {
    if (updating !== null) return;
    updating = link.id;
    actionError = null;
    notice = null;
    try {
      const updated = await api<Link>(`/links/${link.id}`, 'PATCH', { enabled: !link.enabled });
      patchListedLink(updated.id, updated);
      refreshCatalog(false);
      notice = 'linkUpdated';
    } catch (error) {
      actionError = problemMessage(error);
    } finally {
      updating = null;
    }
  }

  async function organize(link: CatalogLink, change: Partial<LinkOrganization>, message: keyof Messages) {
    if (updating !== null) return;
    updating = link.id;
    actionError = null;
    notice = null;
    try {
      const updated = await api<LinkOrganization>(`/links/${link.id}/organization`, 'PATCH', {
        key: link.selection_key,
        ...change,
      });
      patchListedLink(link.id, updated, link.selection_key);
      if (!inCurrentView(updated)) {
        updateSelection(link.selection_key, false);
        viewControl?.focus();
      }
      refreshCatalog(false);
      notice = message;
    } catch (error) {
      actionError = problemMessage(error);
      if (actionError === 'linkOrganizationChanged') refreshCatalog(false);
    } finally {
      updating = null;
    }
  }

  async function remove(event: SubmitEvent) {
    event.preventDefault();
    if (!removing || saving) return;
    const id = removing.id;
    saving = true;
    formError = null;
    actionError = null;
    notice = null;
    try {
      await api<void>(`/links/${id}`, 'DELETE');
      items = items.filter((item) => item.id !== id);
      refreshCatalog();
      removing = null;
      notice = 'linkDeleted';
    } catch (error) {
      formError = problemMessage(error);
    } finally {
      saving = false;
    }
  }

  async function copy(link: Link) {
    notice = null;
    actionError = null;
    try {
      await navigator.clipboard.writeText(shortUrl(link, domains));
      notice = 'copiedUrl';
    } catch {
      actionError = 'copyFailed';
    }
  }

  function exportVisible() {
    const listed = items.map(({ selection_key: _key, favorite: _favorite, archived: _archived, ...link }) => link);
    const data = new Blob([JSON.stringify(listed, null, 2) + '\n'], { type: 'application/json' });
    downloadFile(data, 'shortcircuit-links.json');
  }
</script>

<div class="page-head">
  <div><h1>{t.links}</h1><p>{t.linksDescription}</p></div>
  <button class="button primary" type="button" disabled={bulkBusy} onclick={() => openForm()}><Icon name="plus" />{t.addLink}</button>
</div>

{#if notice}<p class="notice page-notice" role="status">{t[notice]}</p>{/if}
{#if actionError}<p class="notice error page-notice" role="alert">{t[actionError]}</p>{/if}

<section class="panel" aria-label={t.links} aria-busy={loading}>
  <div class="toolbar">
    <label class="search">
      <Icon name="search" />
      <span class="visually-hidden">{t.search}</span>
      <input type="search" placeholder={t.search} maxlength="4096" bind:value={search} />
    </label>
    <label>
      <span class="visually-hidden">{t.linkView}</span>
      <select bind:value={view} bind:this={viewControl}>
        <option value="unarchived">{t.unarchivedLinks}</option>
        <option value="favorites">{t.favoriteLinks}</option>
        <option value="archived">{t.archivedLinks}</option>
      </select>
    </label>
    <label>
      <span class="visually-hidden">{t.domainFilter}</span>
      <select bind:value={domain}>
        <option value="">{t.allDomains}</option>
        {#each availableDomains as option (option)}<option value={option}>{option}</option>{/each}
      </select>
    </label>
    <label>
      <span class="visually-hidden">{t.tagFilter}</span>
      <select disabled={!availableTags.length || tagsFilter.length >= 20} onchange={(event) => {
        if (event.currentTarget.value) toggleTag(event.currentTarget.value);
        event.currentTarget.value = '';
      }}>
        <option value="">{t.addTagFilter}</option>
        {#each availableTags as option (option)}<option value={option}>{option}</option>{/each}
      </select>
    </label>
    <label>
      <span class="visually-hidden">{t.statusFilter}</span>
      <select bind:value={status}>
        <option value="">{t.allStatuses}</option>
        <option value="enabled">{t.enabled}</option>
        <option value="disabled">{t.disabled}</option>
      </select>
    </label>
    <label>
      <span class="visually-hidden">{t.sortLinks}</span>
      <select bind:value={sort}>
        {#each linkSorts as value (value)}<option value={value}>{t[sortLabels[value]]}</option>{/each}
      </select>
    </label>
    <button class="button" type="button" disabled={!items.length} onclick={exportVisible}><Icon name="download" />{t.exportShown}</button>
  </div>

  {#if tagsFilter.length}
    <div class="toolbar tag-filters" role="group" aria-label={t.tagFilter}>
      {#if tagsFilter.length > 1}
        <label>
          <span class="visually-hidden">{t.tagMatch}</span>
          <select bind:value={tagMatch}>
            <option value="all">{t.matchAllTags}</option>
            <option value="any">{t.matchAnyTag}</option>
          </select>
        </label>
      {/if}
      <div class="tags">
        {#each tagsFilter as value (value)}
          <button class="tag filter-tag" type="button" aria-label={`${t.removeTagFilter} ${value}`} onclick={() => toggleTag(value)}>
            {value}<Icon name="close" />
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#if items.length}
    <div class="toolbar selection-toolbar">
      <label class="row">
        <input type="checkbox" bind:this={selectShownInput} checked={allShownSelected} disabled={bulkBusy}
          onchange={(event) => selected = new Set(event.currentTarget.checked ? selectable.map((item) => item.selection_key) : [])} />
        {items.length > maxSelection ? t.bulkSelectFirst50 : t.bulkSelectShown}
      </label>
    </div>
  {/if}
  <BulkLinks {api} {t} {language} selection={selectedLinks} bind:busy={bulkBusy}
    onclear={() => selected = new Set()} onapplied={applyBulk}
    onrefresh={() => { selected = new Set(); refreshCatalog(); }} />

  {#if items.length}
    <div class="table-wrap">
      <!-- svelte-ignore a11y_no_redundant_roles (Mobile card layout overrides native table display.) -->
      <table class="links-table" role="table" aria-label={t.links}>
        <thead>
          <tr><th scope="col">{t.shortLink}</th><th scope="col">{t.tags}</th><th scope="col">{t.status}</th><th scope="col">{t.created}</th><th scope="col" class="align-right">{t.actions}</th></tr>
        </thead>
        <!-- svelte-ignore a11y_no_redundant_roles (Preserve table semantics in mobile cards.) -->
        <tbody role="rowgroup">
          {#each items as link (link.id)}
            <!-- svelte-ignore a11y_no_redundant_roles (Body rows display as grids on mobile.) -->
            <tr role="row">
              <td role="cell">
                <div class="row link-cell">
                  <label class="link-selection">
                    <input type="checkbox" aria-label={`${t.bulkSelectLink} ${link.host}/${link.slug}`}
                      checked={selected.has(link.selection_key)}
                      disabled={bulkBusy || (!selected.has(link.selection_key) && selectedLinks.length >= maxSelection)}
                      onchange={(event) => updateSelection(link.selection_key, event.currentTarget.checked)} />
                  </label>
                  <button class="icon-button ghost favorite-button" type="button" aria-label={t.favoriteLink} title={t.favoriteLink}
                    aria-pressed={link.favorite} disabled={bulkBusy || updating !== null}
                    onclick={() => organize(link, { favorite: !link.favorite }, link.favorite ? 'favoriteRemoved' : 'favoriteAdded')}><Icon name="star" /></button>
                  <div class="link-text">
                    {#if link.title}<strong>{link.title}</strong>{/if}
                    <a class="mono link-short" href={shortUrl(link, domains)} target="_blank" rel="noreferrer">{link.host}/{link.slug}</a>
                    <span class="link-target" title={link.target}>{link.target}</span>
                  </div>
                </div>
              </td>
              <td role="cell">
                <span class="cell-label" aria-hidden="true">{t.tags}</span>
                <div class="tags">
                  {#each link.tags as value (value)}
                    <button class="tag" type="button" aria-pressed={tagsFilter.includes(value)}
                      disabled={tagsFilter.length >= 20 && !tagsFilter.includes(value)} onclick={() => toggleTag(value)}>{value}</button>
                  {/each}
                  {#if !link.tags.length}<span class="muted" aria-hidden="true">—</span>{/if}
                </div>
              </td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.status}</span><span class:active={link.enabled} class="status"><span class="dot"></span>{link.enabled ? t.enabled : t.disabled}</span></td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.created}</span><time datetime={link.created_at}>{formatDate(link.created_at, language)}</time></td>
              <td role="cell">
                <div class="row row-actions">
                  <button class="icon-button ghost" type="button" aria-label={t.copyUrl} title={t.copyUrl} onclick={() => copy(link)}><Icon name="copy" /></button>
                  <button class="icon-button ghost" type="button" disabled={bulkBusy} aria-label={t.editLink} title={t.editLink} onclick={() => openForm(link)}><Icon name="edit" /></button>
                  <button class="icon-button ghost" type="button" disabled={bulkBusy} aria-label={t.redirectPolicy} title={t.redirectPolicy} onclick={() => policyLink = link}><Icon name="rule" /></button>
                  <button class="icon-button ghost" type="button" disabled={bulkBusy} aria-label={t.linkProtection} title={t.linkProtection} onclick={() => protectionLink = link}><Icon name="lock" /></button>
                  <button class="icon-button ghost" type="button" disabled={bulkBusy} aria-label={t.linkHistory} title={t.linkHistory} onclick={() => historyLink = link}><Icon name="history" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.qrCode} title={t.qrCode} onclick={() => qrLink = link}><Icon name="qr" /></button>
                  <button class="button toggle-button" type="button" disabled={bulkBusy || updating !== null} onclick={() => toggle(link)}>{link.enabled ? t.disable : t.enable}</button>
                  <button class="icon-button ghost" type="button" disabled={bulkBusy || updating !== null}
                    aria-label={link.archived ? t.unarchiveLink : t.archiveLink} title={link.archived ? t.unarchiveLink : t.archiveLink}
                    onclick={() => organize(link, { archived: !link.archived }, link.archived ? 'linkUnarchived' : 'linkArchived')}><Icon name={link.archived ? 'unarchive' : 'archive'} /></button>
                  <button class="icon-button ghost delete-button" type="button" disabled={bulkBusy} aria-label={t.deleteLink} title={t.deleteLink} onclick={() => { formError = null; removing = link; }}><Icon name="trash" /></button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !loading && initialized}
    <div class="empty">
      <span class="link-symbol"><Icon name="link" /></span>
      <p>{search || tagsFilter.length || status || domain ? t.noMatches : view === 'archived' ? t.noArchivedLinks : view === 'favorites' ? t.noFavoriteLinks : t.noLinks}</p>
      {#if search || tagsFilter.length || status || domain}<button class="text-button" type="button" onclick={clearFilters}>{t.resetFilters}</button>{/if}
    </div>
  {/if}
  {#if loadError}
    <div class="panel-foot between"><p role="alert" class="notice error">{t[loadError]}</p><button class="button" type="button" onclick={() => retryPage?.()}>{t.retry}</button></div>
  {/if}
  <div class="panel-foot between">
    <p class="table-count" aria-live="polite">
      {#if loading}{t.loading}{:else}{limit.format(items.length)} {t.loaded}{/if}
    </p>
    {#if cursor !== null && !loadError}<button class="button" type="button" disabled={loading} onclick={loadMore}>{t.loadMore}</button>{/if}
  </div>
</section>

{#if qrLink}<LinkQr {api} link={qrLink} {t} onclose={() => qrLink = null} />{/if}
{#if historyLink}
  <LinkHistory link={historyLink} {api} {t} {language} onclose={() => historyLink = null} onrestore={restoreMetadata} />
{/if}
{#if policyLink}
  <PolicyEditor link={policyLink} {api} {t} onclose={() => policyLink = null} onsaved={() => { policyLink = null; notice = 'linkUpdated'; }} />
{/if}
{#if protectionLink}
  <LinkProtectionEditor link={protectionLink} {api} {t} onclose={() => protectionLink = null}
    onsaved={() => { protectionLink = null; notice = 'linkProtectionSaved'; }} />
{/if}

<Dialog {t} title={editing ? t.editLink : t.addLink} open={formOpen} busy={saving} onclose={closeForm}>
  <form onsubmit={save} aria-busy={saving}>
    <div class="dialog-body stack">
      {#if editing}<a class="mono link-short" href={shortUrl(editing, domains)} target="_blank" rel="noreferrer">{shortUrl(editing, domains)}</a>{/if}
      <label class="field">
        <span id="target-label">{t.target}</span>
        <input name="target" type="url" required placeholder="https://example.com/" bind:value={target} disabled={saving} aria-labelledby="target-label" aria-describedby="target-hint" aria-invalid={formError === 'invalidTarget'} />
        <small id="target-hint">{t.targetHint}</small>
      </label>
      {#if formOpen && !saving}
        <UrlCleaner {t} value={target} onapply={(value) => target = value} />
        <DuplicateLookup {t} {api} {domains} value={target} excludeId={editing?.id} />
      {/if}
      <label class="field">
        <span>{t.title}</span><input name="title" bind:value={title} disabled={saving} aria-invalid={formError === 'invalidTitle'} />
      </label>
      {#if !editing}
        <label class="field">
          <span>{t.domain}</span>
          <select name="host" bind:value={host} disabled={saving}>
            {#each domains.hosts as value (value)}<option value={value}>{value}</option>{/each}
          </select>
        </label>
        <label class="field">
          <span id="slug-label">{t.slug}</span>
          <input name="slug" maxlength="200" autocapitalize="none" spellcheck="false" bind:value={slug} disabled={saving} aria-labelledby="slug-label" aria-describedby="slug-hint" aria-invalid={formError === 'invalidSlug' || formError === 'slugInUse'} />
          <small id="slug-hint">{t.slugHint}</small>
        </label>
      {/if}
      <label class="field">
        <span id="tags-label">{t.tags}</span>
        <input name="tags" bind:value={tags} disabled={saving} aria-labelledby="tags-label" aria-describedby="tags-hint" aria-invalid={formError === 'invalidTags'} />
        <small id="tags-hint">{t.tagsHint}</small>
      </label>
      {#if editing}<label class="row"><input type="checkbox" bind:checked={analyticsEnabled} disabled={saving} />{t.countLinkVisits}</label>{/if}
      {#if formOpen && !editing}
        <details class="protection-options">
          <summary>{t.linkProtection}</summary>
          <ProtectionFields {t} bind:oneTime bind:password={linkPassword} busy={saving}
            invalidPassword={formError === 'invalidPassword'} />
        </details>
      {/if}
      {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
    </div>
    <footer class="dialog-foot row">
      <button class="button" type="button" disabled={saving} onclick={closeForm}>{t.cancel}</button>
      <button class="button primary" type="submit" disabled={saving}>{saving ? t.saving : editing ? t.save : t.addLink}</button>
    </footer>
  </form>
</Dialog>

<Dialog {t} title={t.deleteLink} open={removing !== null} busy={saving} onclose={() => removing = null}>
  {#if removing}
    <form onsubmit={remove} aria-busy={saving}>
      <div class="dialog-body stack compact">
        <strong>{linkName(removing)}</strong>
        <p>{t.deleteLinkHint}</p>
        {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
      </div>
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={saving} onclick={() => removing = null}>{t.cancel}</button>
        <button class="button danger" type="submit" disabled={saving}>{saving ? t.saving : t.delete}</button>
      </footer>
    </form>
  {/if}
</Dialog>

<style>
  .selection-toolbar {
    border-top: 1px solid var(--line);
  }

  .link-selection {
    display: grid;
    place-items: center;
    flex: 0 0 var(--hit);
    min-block-size: var(--hit);
  }

  .protection-options { border: 1px solid var(--line); border-radius: 8px; padding: 0 12px; }
  .protection-options[open] { padding-bottom: 12px; }
  .protection-options summary { padding-block: 11px; cursor: pointer; font-weight: 650; }
</style>
