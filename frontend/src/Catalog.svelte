<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Api, Domain, Tag } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import TagMark from './TagMark.svelte';
  import type { Language, Messages } from './i18n';

  let { t, api, language, kind, onDomainSaved }: {
    t: Messages; api: Api; language: Language; kind: 'domains' | 'tags'; onDomainSaved: () => void;
  } = $props();

  let domains = $state<Domain[]>([]);
  let tags = $state<Tag[]>([]);
  let loading = $state(false);
  let error = $state<keyof Messages | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let action = $state<'domain' | 'rename' | 'tag-details' | 'delete' | null>(null);
  let saving = $state(false);
  let existing = $state(false);
  let domain = $state<Domain>();
  let tag = $state('');
  let newName = $state('');
  let useTagColor = $state(false);
  let tagColor = $state('#60a5fa');
  let tagDescription = $state('');
  let heading: HTMLHeadingElement;
  const count = $derived(new Intl.NumberFormat(language));

  onMount(() => { void load(); });

  async function load(showLoading = true) {
    loading = showLoading;
    error = null;
    try {
      if (kind === 'domains') domains = (await api<{ items: Domain[] }>('/catalog/domains')).items;
      else tags = (await api<{ items: Tag[] }>('/catalog/tags')).items;
    } catch (result) {
      error = problemMessage(result);
    } finally {
      loading = false;
    }
  }

  function editDomain(item?: Domain) {
    existing = Boolean(item);
    domain = item ? { ...item } : { host: '', origin: '', enabled: true, root_target: null, missing_target: null };
    formError = null;
    action = 'domain';
  }

  function editTag(item: Tag, mode: 'rename' | 'tag-details' | 'delete') {
    tag = item.name;
    newName = item.name;
    useTagColor = Boolean(item.color);
    tagColor = item.color ?? '#60a5fa';
    tagDescription = item.description ?? '';
    formError = null;
    action = mode;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    saving = true;
    formError = null;
    let focusHeading = false;
    try {
      if (action === 'domain' && domain) {
        const updated = await api<Domain>('/catalog/domains', 'POST', {
          ...domain,
          host: domain.host.trim(),
          origin: domain.origin.trim(),
          root_target: domain.root_target?.trim() || null,
          missing_target: domain.missing_target?.trim() || null,
        });
        domains = domains.some((item) => item.host === updated.host)
          ? domains.map((item) => item.host === updated.host ? updated : item)
          : [...domains, updated];
        onDomainSaved();
      } else if (action === 'rename') {
        await api('/catalog/tags', 'PATCH', { name: tag, new_name: newName.trim() });
        await load(false);
        focusHeading = Boolean(error) || !tags.some((item) => item.name === tag);
      } else if (action === 'tag-details') {
        if ([...tagDescription.trim()].length > 500) {
          formError = 'invalidDescription';
          return;
        }
        await api('/catalog/tags/details', 'PATCH', {
          name: tag, color: useTagColor ? tagColor : null, description: tagDescription.trim(),
        });
        await load(false);
        focusHeading = Boolean(error) || !tags.some((item) => item.name === tag);
      } else if (action === 'delete') {
        await api('/catalog/tags', 'DELETE', { name: tag });
        tags = tags.filter((item) => item.name !== tag);
        focusHeading = true;
      }
      action = null;
      if (focusHeading) {
        await tick();
        heading.focus();
      }
    } catch (result) {
      formError = problemMessage(result);
    } finally {
      saving = false;
    }
  }
</script>

<div class="page-head">
  <div>
    <h1 tabindex="-1" bind:this={heading}>{t[kind]}</h1>
    {#if kind === 'domains'}<p>{t.domainsDescription}</p>{/if}
  </div>
  {#if kind === 'domains'}
    <button class="button primary" type="button" disabled={loading} onclick={() => editDomain()}><Icon name="plus" />{t.addDomain}</button>
  {/if}
</div>

<section class="panel panel-pad stack" aria-label={t[kind]} aria-busy={loading}>
  {#if error}
    <p class="notice error" role="alert">{t[error]}</p>
    <div><button class="button" type="button" onclick={() => load()}>{t.retry}</button></div>
  {:else if loading}
    <p role="status">{t.loading}</p>
  {:else if kind === 'domains'}
    {#each domains as item (item.host)}
      <div class="catalog-row between">
        <div class="stack compact catalog-label">
          <strong>{item.host}</strong>
          <span class="mono">{item.origin}</span>
        </div>
        <div class="row catalog-actions">
          <span class:active={item.enabled} class="status"><span class="dot"></span>{item.enabled ? t.enabled : t.disabled}</span>
          <button class="button" type="button" onclick={() => editDomain(item)}>{t.editDomain}</button>
        </div>
      </div>
    {/each}
  {:else if tags.length}
    {#each tags as item (item.name)}
      <div class="catalog-row between">
        <div class="stack compact catalog-label">
          <strong class="tag-name"><TagMark color={item.color} />{item.name}</strong>
          <span>{count.format(item.link_count)} · {t.links}</span>
          {#if item.description}<p class="tag-description">{item.description}</p>{/if}
        </div>
        <div class="row catalog-actions">
          <button class="button" type="button" onclick={() => editTag(item, 'tag-details')}>{t.editTagDetails}</button>
          <button class="button" type="button" onclick={() => editTag(item, 'rename')}>{t.renameTag}</button>
          <button class="icon-button ghost delete-button" type="button" aria-label={t.deleteTag} title={t.deleteTag} onclick={() => editTag(item, 'delete')}><Icon name="trash" /></button>
        </div>
      </div>
    {/each}
  {:else}
    <p>{t.noTags}</p>
  {/if}
</section>

<Dialog {t} title={action === 'rename' ? t.renameTag : action === 'tag-details' ? t.editTagDetails : action === 'delete' ? t.deleteTag : existing ? t.editDomain : t.addDomain}
  open={action !== null} busy={saving} onclose={() => action = null}>
  <form onsubmit={save} aria-busy={saving}>
    <div class="dialog-body stack">
      {#if action === 'domain' && domain}
        <label class="field"><span>{t.domain}</span><input bind:value={domain.host} required readonly={existing} disabled={saving} placeholder="go.example.com" /></label>
        <label class="field"><span>{t.publicOrigin}</span><input type="url" bind:value={domain.origin} required disabled={saving} placeholder="https://go.example.com" /></label>
        <label class="row"><input type="checkbox" bind:checked={domain.enabled} disabled={saving} />{t.enabled}</label>
        <label class="field"><span>{t.rootTarget}</span><input type="url" bind:value={domain.root_target} disabled={saving} /></label>
        <label class="field"><span>{t.missingTarget}</span><input type="url" bind:value={domain.missing_target} disabled={saving} /></label>
      {:else if action === 'rename'}
        <label class="field"><span>{t.tagName}</span><input bind:value={newName} required maxlength="40" disabled={saving} /></label>
        <p>{t.mergeTagHint}</p>
      {:else if action === 'tag-details'}
        <strong>{tag}</strong>
        <label class="row"><input type="checkbox" bind:checked={useTagColor} disabled={saving} />{t.useTagColor}</label>
        {#if useTagColor}
          <label class="row"><input class="tag-color-input" type="color" bind:value={tagColor} disabled={saving} />{t.tagColor}</label>
        {/if}
        <label class="field">
          <span>{t.tagDescription}</span>
          <textarea rows="2" maxlength="1000" bind:value={tagDescription} disabled={saving}></textarea>
        </label>
      {:else if action === 'delete'}
        <strong>{tag}</strong><p>{t.deleteTagHint}</p>
      {/if}
      {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
    </div>
    <footer class="dialog-foot row">
      <button class="button" type="button" disabled={saving} onclick={() => action = null}>{t.cancel}</button>
      <button class="button" class:primary={action !== 'delete'} class:danger={action === 'delete'} type="submit" disabled={saving}>
        {saving ? t.saving : action === 'delete' ? t.delete : t.save}
      </button>
    </footer>
  </form>
</Dialog>

<style>
  .catalog-row { gap: 12px; flex-wrap: wrap; }
  .catalog-row + .catalog-row { padding-block-start: 12px; border-block-start: 1px solid var(--line); }
  .catalog-label { overflow-wrap: anywhere; min-inline-size: 0; }
  .catalog-label span { color: var(--muted); font-size: .875rem; }
  .catalog-actions { flex-wrap: wrap; }
  .tag-name { display: flex; align-items: center; gap: 8px; }
  .tag-description { white-space: pre-line; }
  .tag-color-input { width: var(--hit); padding: 4px; }
</style>
