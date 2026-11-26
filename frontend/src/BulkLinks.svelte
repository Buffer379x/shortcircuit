<script lang="ts">
  import { ApiError } from './api';
  import type { Api, BulkLinkResult, CatalogLink } from './api';
  import Dialog from './Dialog.svelte';
  import { downloadFile, problemMessage } from './format';
  import type { Language, Messages } from './i18n';

  let { api, t, language, selection, busy = $bindable(false), onclear, onapplied, onrefresh }: {
    api: Api;
    t: Messages;
    language: Language;
    selection: CatalogLink[];
    busy: boolean;
    onclear: () => void;
    onapplied: (result: BulkLinkResult, items: CatalogLink[]) => void;
    onrefresh: () => void;
  } = $props();

  const choices = {
    enable: 'bulkEnable', disable: 'bulkDisable', add_tags: 'bulkAddTags', remove_tags: 'bulkRemoveTags',
    export_json: 'bulkExportJson', export_csv: 'bulkExportCsv', delete: 'bulkDelete',
  } as const satisfies Record<string, keyof Messages>;
  type Choice = keyof typeof choices;
  type Change =
    | { action: 'set_enabled'; enabled: boolean }
    | { action: 'add_tags' | 'remove_tags'; tags: string[] }
    | { action: 'delete'; confirmed: true };
  let choice = $state<Choice>('enable');
  let tags = $state('');
  let confirmation = $state.raw<CatalogLink[] | null>(null);
  let error = $state<keyof Messages | null>(null);
  const needsTags = $derived(choice === 'add_tags' || choice === 'remove_tags');
  const number = $derived(new Intl.NumberFormat(language));
  const path = '/links/bulk';

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !selection.length) return;
    const items = [...selection];
    error = null;
    switch (choice) {
      case 'enable':
      case 'disable':
        void apply(items, { action: 'set_enabled', enabled: choice === 'enable' });
        break;
      case 'add_tags':
      case 'remove_tags':
        void apply(items, { action: choice, tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean) });
        break;
      case 'export_json':
      case 'export_csv':
        void exportSelection(items, choice === 'export_json' ? 'json' : 'csv');
        break;
      case 'delete':
        confirmation = items;
        break;
    }
  }

  function identities(items: CatalogLink[]) {
    return items.map(({ id, selection_key }) => ({ id, key: selection_key }));
  }

  async function apply(items: CatalogLink[], change: Change) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const result = await api<BulkLinkResult>(path, 'POST', { links: identities(items), change });
      confirmation = null;
      tags = '';
      choice = 'enable';
      onapplied(result, items);
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === 'bulk_selection_changed') confirmation = null;
      error = problemMessage(failure);
    } finally {
      busy = false;
    }
  }

  async function exportSelection(items: CatalogLink[], format: 'json' | 'csv') {
    busy = true;
    try {
      const body = { links: identities(items), format };
      const file = format === 'csv'
        ? await (await api<Response>(path + '/export', 'POST', body, 'csv')).blob()
        : new Blob([JSON.stringify(await api<unknown>(path + '/export', 'POST', body))], { type: 'application/json' });
      downloadFile(file, `shortcircuit-selected-links.${format}`);
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      busy = false;
    }
  }

  function clearSelection() {
    error = null;
    tags = '';
    choice = 'enable';
    onclear();
  }

  function reload() {
    error = null;
    onrefresh();
  }
</script>

{#if selection.length}
  <form class="toolbar bulk-actions" aria-label={t.bulkActions} aria-busy={busy} onsubmit={submit}>
    <span role="status">{number.format(selection.length)} {t.bulkSelected}</span>
    <label>
      <span class="visually-hidden">{t.bulkActions}</span>
      <select bind:value={choice} disabled={busy} onchange={() => { error = null; }}>
        {#each Object.keys(choices) as key (key)}
          <option value={key}>{t[choices[key as Choice]]}</option>
        {/each}
      </select>
    </label>
    {#if needsTags}
      <label>
        <span class="visually-hidden">{t.bulkTagsInput}</span>
        <input bind:value={tags} placeholder={t.bulkTagsInput} required disabled={busy} />
      </label>
    {/if}
    <button type="submit" class="button primary" disabled={busy || (needsTags && !tags.trim())}>{t.bulkApply}</button>
    <button type="button" class="text-button" disabled={busy} onclick={clearSelection}>{t.bulkClear}</button>
    {#if error && !confirmation}
      <div class="row bulk-error">
        <p role="alert" class="notice error">{t[error]}</p>
        {#if error === 'bulkSelectionChanged'}
          <button class="button" type="button" onclick={reload}>{t.bulkReload}</button>
        {/if}
      </div>
    {/if}
  </form>
{/if}

<Dialog {t} title={t.bulkDeleteTitle} open={confirmation !== null} {busy} onclose={() => confirmation = null}>
  {#if confirmation}
    <div class="dialog-body stack">
      <p>{t.bulkDeleteHint}</p>
      <ul class="bulk-delete-list">
        {#each confirmation as item (item.selection_key)}
          <li class="mono">{item.host}/{item.slug}</li>
        {/each}
      </ul>
      {#if error}<p role="alert" class="notice error">{t[error]}</p>{/if}
    </div>
    <footer class="dialog-foot row">
      <button class="button" type="button" disabled={busy} onclick={() => confirmation = null}>{t.cancel}</button>
      <button class="button danger" type="button" disabled={busy}
        onclick={() => { if (confirmation) void apply(confirmation, { action: 'delete', confirmed: true }); }}>{t.bulkDelete}</button>
    </footer>
  {/if}
</Dialog>

<style>
  .bulk-actions {
    border-top: 1px solid var(--line);
  }

  .bulk-actions input {
    max-inline-size: 100%;
  }

  .bulk-error {
    flex-wrap: wrap;
    width: 100%;
  }

  .bulk-delete-list {
    margin: 0;
    padding-inline-start: 1.4rem;
    max-block-size: min(40dvh, 18rem);
    overflow: auto;
    overflow-wrap: anywhere;
  }
</style>
