<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api, Domain, Tag } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  let { t, api, language, kind, onDomainSaved }: {
    t: Messages; api: Api; language: Language; kind: 'domains' | 'tags'; onDomainSaved: () => void;
  } = $props();

  let domains = $state<Domain[]>([]);
  let tags = $state<Tag[]>([]);
  let loading = $state(false);
  let error = $state<keyof Messages | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let action = $state<'domain' | 'rename' | 'delete' | null>(null);
  let saving = $state(false);
  let existing = $state(false);
  let domain = $state<Domain>();
  let tag = $state('');
  let newName = $state('');
  const count = $derived(new Intl.NumberFormat(language));

  onMount(() => { void load(); });

  async function load() {
    loading = true;
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

  function editTag(name: string, mode: 'rename' | 'delete') {
    tag = name;
    newName = name;
    formError = null;
    action = mode;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    saving = true;
    formError = null;
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
        await load();
      } else if (action === 'delete') {
        await api('/catalog/tags', 'DELETE', { name: tag });
        tags = tags.filter((item) => item.name !== tag);
      }
      action = null;
    } catch (result) {
      formError = problemMessage(result);
    } finally {
      saving = false;
    }
  }
</script>

<div class="page-head">
  <div>
    <h1>{t[kind]}</h1>
    {#if kind === 'domains'}<p>{t.domainsDescription}</p>{/if}
  </div>
  {#if kind === 'domains'}
    <button class="button primary" type="button" disabled={loading} onclick={() => editDomain()}><Icon name="plus" />{t.addDomain}</button>
  {/if}
</div>

<section class="panel panel-pad stack" aria-label={t[kind]} aria-busy={loading}>
  {#if error}
    <p class="notice error" role="alert">{t[error]}</p>
    <div><button class="button" type="button" onclick={load}>{t.retry}</button></div>
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
          <strong>{item.name}</strong>
          <span>{count.format(item.link_count)} · {t.links}</span>
        </div>
        <div class="row catalog-actions">
          <button class="button" type="button" onclick={() => editTag(item.name, 'rename')}>{t.renameTag}</button>
          <button class="icon-button ghost delete-button" type="button" aria-label={t.deleteTag} title={t.deleteTag} onclick={() => editTag(item.name, 'delete')}><Icon name="trash" /></button>
        </div>
      </div>
    {/each}
  {:else}
    <p>{t.noTags}</p>
  {/if}
</section>

<Dialog {t} title={action === 'rename' ? t.renameTag : action === 'delete' ? t.deleteTag : existing ? t.editDomain : t.addDomain}
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
</style>
