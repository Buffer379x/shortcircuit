<script lang="ts">
  import { onMount } from 'svelte';
  import { linkName, linkSorts, shortUrl } from './api';
  import type { Api, Domains, Link, LinkCursor, LinkProtectionChange, LinkSearch, LinkSearchPage, LinkSort, Tag } from './api';
  import Dialog from './Dialog.svelte';
  import { formatDate, passwordAllowed, problemMessage } from './format';
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

  let items = $state<Link[]>([]);
  let cursor = $state.raw<LinkCursor | null>(null);
  let loading = $state(false);
  let initialized = $state(false);
  let loadError = $state<keyof Messages | null>(null);
  let search = $state('');
  let tag = $state('');
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

  const availableTags = $derived([...new Set([...catalogTags, ...items.flatMap((link) => link.tags)])].sort((a, b) => a.localeCompare(b, language)));
  const availableDomains = $derived(Object.keys(domains.origins).sort());
  const limit = $derived(new Intl.NumberFormat(language));
  const tagRequests = new AbortController();
  let tagGeneration = 0;
  let currentQuery: LinkSearch = { sort: 'newest' };
  let pageRequest: AbortController | null = null;
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
    void revision;
    const criteria: LinkSearch = {
      query: search.trim() || undefined,
      host: domain || undefined,
      tag: tag || undefined,
      enabled: status ? status === 'enabled' : undefined,
      sort,
    };
    const pending = new AbortController();
    pageRequest = pending;
    currentQuery = criteria;
    items = [];
    cursor = null;
    initialized = false;
    loading = true;
    loadError = null;
    const timer = setTimeout(() => { void loadPage(criteria, null, pending.signal); }, 150);
    return () => {
      clearTimeout(timer);
      pending.abort();
    };
  });

  async function loadTags() {
    const generation = ++tagGeneration;
    try {
      const page = await api<{ items: Tag[] }>('/catalog/tags', 'GET', undefined, 'json', tagRequests.signal);
      if (!tagRequests.signal.aborted && generation === tagGeneration) catalogTags = page.items.map(({ name }) => name);
    } catch {
      // Tags from the fetched links remain usable when the catalog is unavailable.
    }
  }

  async function loadPage(criteria: LinkSearch, after: LinkCursor | null, signal: AbortSignal) {
    loading = true;
    loadError = null;
    try {
      const page = await api<LinkSearchPage>('/links/search', 'POST', { ...criteria, after: after ?? undefined }, 'json', signal);
      if (signal.aborted) return;
      const seen = new Set(items.map((link) => link.id));
      items = [...items, ...page.items.filter((link) => !seen.has(link.id))];
      cursor = page.next_cursor;
      initialized = true;
    } catch (error) {
      if (!signal.aborted) loadError = problemMessage(error);
    } finally {
      if (!signal.aborted) loading = false;
    }
  }

  function loadMore() {
    if (!loading && pageRequest) void loadPage(currentQuery, cursor, pageRequest.signal);
  }

  function refreshCatalog() {
    revision += 1;
    void loadTags();
  }

  function clearFilters() {
    search = '';
    tag = '';
    domain = '';
    status = '';
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
        await api<Link>(`/links/${editing.id}`, 'PATCH', changes);
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
      await api<Link>(`/links/${link.id}`, 'PATCH', { enabled: !link.enabled });
      refreshCatalog();
      notice = 'linkUpdated';
    } catch (error) {
      actionError = problemMessage(error);
    } finally {
      updating = null;
    }
  }

  async function remove(event: SubmitEvent) {
    event.preventDefault();
    if (!removing || saving) return;
    saving = true;
    formError = null;
    actionError = null;
    notice = null;
    try {
      await api<void>(`/links/${removing.id}`, 'DELETE');
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
    const data = new Blob([JSON.stringify(items, null, 2) + '\n'], { type: 'application/json' });
    const url = URL.createObjectURL(data);
    const download = document.createElement('a');
    download.href = url;
    download.download = 'shortcircuit-links.json';
    download.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
</script>

<div class="page-head">
  <div><h1>{t.links}</h1><p>{t.linksDescription}</p></div>
  <button class="button primary" type="button" onclick={() => openForm()}><Icon name="plus" />{t.addLink}</button>
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
      <span class="visually-hidden">{t.domainFilter}</span>
      <select bind:value={domain}>
        <option value="">{t.allDomains}</option>
        {#each availableDomains as option (option)}<option value={option}>{option}</option>{/each}
      </select>
    </label>
    <label>
      <span class="visually-hidden">{t.tagFilter}</span>
      <select bind:value={tag}>
        <option value="">{t.allTags}</option>
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
                  <span class="link-symbol"><Icon name="link" /></span>
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
                  {#each link.tags as value (value)}<button class="tag" type="button" aria-pressed={tag === value} onclick={() => tag = tag === value ? '' : value}>{value}</button>{/each}
                  {#if !link.tags.length}<span class="muted" aria-hidden="true">—</span>{/if}
                </div>
              </td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.status}</span><span class:active={link.enabled} class="status"><span class="dot"></span>{link.enabled ? t.enabled : t.disabled}</span></td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.created}</span><time datetime={link.created_at}>{formatDate(link.created_at, language)}</time></td>
              <td role="cell">
                <div class="row row-actions">
                  <button class="icon-button ghost" type="button" aria-label={t.copyUrl} title={t.copyUrl} onclick={() => copy(link)}><Icon name="copy" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.editLink} title={t.editLink} onclick={() => openForm(link)}><Icon name="edit" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.redirectPolicy} title={t.redirectPolicy} onclick={() => policyLink = link}><Icon name="rule" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.linkProtection} title={t.linkProtection} onclick={() => protectionLink = link}><Icon name="lock" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.linkHistory} title={t.linkHistory} onclick={() => historyLink = link}><Icon name="history" /></button>
                  <button class="icon-button ghost" type="button" aria-label={t.qrCode} title={t.qrCode} onclick={() => qrLink = link}><Icon name="qr" /></button>
                  <button class="button toggle-button" type="button" disabled={updating !== null} onclick={() => toggle(link)}>{link.enabled ? t.disable : t.enable}</button>
                  <button class="icon-button ghost delete-button" type="button" aria-label={t.deleteLink} title={t.deleteLink} onclick={() => { formError = null; removing = link; }}><Icon name="trash" /></button>
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
      <p>{search || tag || status || domain ? t.noMatches : t.noLinks}</p>
      {#if search || tag || status || domain}<button class="text-button" type="button" onclick={clearFilters}>{t.resetFilters}</button>{/if}
    </div>
  {/if}
  {#if loadError}
    <div class="panel-foot between"><p role="alert" class="notice error">{t[loadError]}</p><button class="button" type="button" onclick={loadMore}>{t.retry}</button></div>
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
  <LinkHistory link={historyLink} {api} {t} {language} onclose={() => historyLink = null} onrestore={refreshCatalog} />
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
  .protection-options { border: 1px solid var(--line); border-radius: 8px; padding: 0 12px; }
  .protection-options[open] { padding-bottom: 12px; }
  .protection-options summary { padding-block: 11px; cursor: pointer; font-weight: 650; }
</style>
