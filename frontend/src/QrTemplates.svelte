<script lang="ts">
  import { ApiError } from './api';
  import type { Api, QrStyle, QrTemplate } from './api';
  import { problemMessage } from './format';
  import type { Messages } from './i18n';

  let { api, t, style, canSave, disabled, onapply }: {
    api: Api;
    t: Messages;
    style: QrStyle;
    canSave: boolean;
    disabled: boolean;
    onapply: (style: QrStyle) => void;
  } = $props();

  const path = '/qr/templates';
  let items = $state.raw<QrTemplate[] | null>(null);
  let selectedId = $state<number | null>(null);
  let name = $state('');
  let busy = $state(false);
  let error = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);
  const selected = $derived(items?.find((item) => item.id === selectedId));
  const locked = $derived(disabled || busy);

  async function load() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      items = (await api<{ items: QrTemplate[] }>(path)).items;
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      busy = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    const label = name.trim();
    if (locked || !canSave || !items || !label) return;
    error = null;
    notice = null;
    busy = true;
    try {
      const item = await api<QrTemplate>(path, 'POST', { ...style, name: label });
      items = [item, ...items];
      selectedId = item.id;
      name = '';
      notice = 'qrTemplateSaved';
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      busy = false;
    }
  }

  function apply() {
    if (locked || !selected) return;
    error = null;
    notice = null;
    onapply(selected);
  }

  async function remove() {
    if (locked || !selected || !items) return;
    const id = selected.id;
    error = null;
    notice = null;
    busy = true;
    try {
      await api<void>(`${path}/${id}`, 'DELETE');
    } catch (failure) {
      if (!(failure instanceof ApiError) || failure.status !== 404) {
        error = problemMessage(failure);
        return;
      }
    } finally {
      busy = false;
    }
    items = items.filter((item) => item.id !== id);
    selectedId = null;
    notice = 'qrTemplateDeleted';
  }
</script>

<details ontoggle={(event) => { if (event.currentTarget.open && items === null) void load(); }}>
  <summary>{t.qrTemplates}</summary>
  <div class="stack compact" aria-busy={busy}>
    {#if items}
      {#if items.length}
        <div class="row template-row">
          <label class="field">
            <span>{t.qrSavedTemplate}</span>
            <select bind:value={selectedId} disabled={locked}>
              <option value={null}>{t.qrSelectTemplate}</option>
              {#each items as item (item.id)}
                <option value={item.id}>{item.name}</option>
              {/each}
            </select>
          </label>
          <button class="button" type="button" disabled={locked || !selected} onclick={apply}>{t.qrApplyTemplate}</button>
          <button class="button" type="button" disabled={locked || !selected} onclick={remove}>{t.qrDeleteTemplate}</button>
        </div>
      {/if}
      <form class="row template-row" onsubmit={save}>
        <label class="field">
          <span>{t.qrTemplateName}</span>
          <input type="text" bind:value={name} maxlength="128" required disabled={locked} />
        </label>
        <button class="button" type="submit" disabled={locked || !canSave || !name.trim()}>{t.qrSaveTemplate}</button>
      </form>
    {:else if busy}
      <p role="status">{t.loading}</p>
    {:else if error}
      <button class="button" type="button" onclick={load}>{t.retry}</button>
    {/if}
    {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
    {#if notice}<p class="notice" role="status">{t[notice]}</p>{/if}
  </div>
</details>

<style>
  summary {
    cursor: pointer;
    min-block-size: var(--hit);
    align-content: center;
    font-weight: 620;
  }

  .template-row {
    align-items: end;
    flex-wrap: wrap;
  }

  .template-row .field {
    flex: 1 1 10rem;
    min-inline-size: 0;
  }
</style>
