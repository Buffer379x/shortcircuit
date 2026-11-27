<script lang="ts">
  // Tags live next to the link list because they organize links; they are not a separate area.
  // Renaming to an existing name merges the tags; the destination keeps its own details.
  import { onMount, tick } from 'svelte';
  import type { Tag } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import { app, tagColor } from '../lib/app.svelte';
  import { linksChanged } from '../lib/overlays.svelte';
  import { num } from '../lib/fmt';
  import { href } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tr } from '../lib/locale.svelte';
  import { tip } from '../lib/tooltip';

  let loading = $state(app.tags.length === 0);
  let loadError = $state<keyof Messages | null>(null);
  let busy = $state(false);
  let formError = $state<{ field: 'name' | 'description' | 'color' | 'form'; message: keyof Messages } | null>(null);

  let editing = $state<Tag | null>(null);
  let name = $state('');
  let description = $state('');
  let color = $state<string>('');
  let custom = $state('');
  let swatches = $state<string[]>([]);
  let mergeFrom = $state<Tag | null>(null);
  let mergeInto = $state('');
  let removing = $state<Tag | null>(null);

  onMount(() => void load());

  async function load() {
    loadError = null;
    try {
      app.tags = (await app.api<{ items: Tag[] }>('/catalog/tags')).items;
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  // Swatches are the chart palette of the current theme, stored as plain hex colors on the tag.
  function palette() {
    const css = getComputedStyle(document.documentElement);
    return [1, 2, 3, 4, 5, 6].map((i) => css.getPropertyValue(`--chart-${i}`).trim().toLowerCase()).filter((v) => /^#[\da-f]{6}$/.test(v));
  }

  function edit(t: Tag) {
    editing = t;
    name = t.name;
    description = t.description ?? '';
    color = t.color ?? '';
    swatches = palette();
    custom = t.color && !swatches.includes(t.color) ? t.color : swatches[0] ?? '';
    formError = null;
  }

  const merging = $derived(!!editing && name.trim() !== editing.name && app.tags.some((t) => t.name === name.trim()));

  async function after(focusHeading: boolean) {
    await load();
    linksChanged();
    if (focusHeading) {
      await tick();
      document.querySelector<HTMLElement>('main h1')?.focus();
    }
  }

  async function save(ev: SubmitEvent) {
    ev.preventDefault();
    if (!editing || busy) return;
    const next = name.trim();
    const text = description.trim();
    if (!next) {
      formError = { field: 'name', message: 'invalidTags' };
      return focusError();
    }
    if ([...text].length > 500) {
      formError = { field: 'description', message: 'invalidDescription' };
      return focusError();
    }
    busy = true;
    formError = null;
    const original = editing;
    try {
      if ((color || null) !== (original.color ?? null) || text !== (original.description ?? '')) {
        await app.api('/catalog/tags/details', 'PATCH', { name: original.name, color: color || null, description: text });
      }
      if (next !== original.name) await app.api('/catalog/tags', 'PATCH', { name: original.name, new_name: next });
      editing = null;
      toast(tr(merging ? 'lnkTagsMerged' : 'lnkTagSaved'), { body: next });
      await after(next !== original.name);
    } catch (error) {
      const message = problemMessage(error);
      formError = { field: message === 'invalidTags' ? 'name' : message === 'invalidDescription' ? 'description' : message === 'invalidTagColor' ? 'color' : 'form', message };
      void focusError();
    } finally {
      busy = false;
    }
  }

  async function focusError() {
    await tick();
    const id = formError?.field === 'name' ? 'tg-n' : formError?.field === 'description' ? 'tg-d' : formError?.field === 'color' ? 'tg-c-none' : 'tg-e';
    document.getElementById(id)?.focus();
  }

  async function merge(ev: SubmitEvent) {
    ev.preventDefault();
    if (!mergeFrom || !mergeInto || busy) return;
    busy = true;
    formError = null;
    const from = mergeFrom.name;
    try {
      await app.api('/catalog/tags', 'PATCH', { name: from, new_name: mergeInto });
      mergeFrom = null;
      toast(tr('lnkTagsMerged'), { body: `${from} → ${mergeInto}` });
      await after(true);
    } catch (error) {
      formError = { field: 'form', message: problemMessage(error) };
    } finally {
      busy = false;
    }
  }

  async function remove(ev: SubmitEvent) {
    ev.preventDefault();
    if (!removing || busy) return;
    busy = true;
    formError = null;
    const t = removing;
    try {
      await app.api('/catalog/tags', 'DELETE', { name: t.name });
      removing = null;
      toast(tr('lnkTagRemoved'), { body: tr('lnkTagRemovedBody', { name: t.name, count: num(t.link_count) }) });
      await after(true);
    } catch (error) {
      formError = { field: 'form', message: problemMessage(error) };
    } finally {
      busy = false;
    }
  }
</script>

{#snippet formNotice()}
  {#if formError?.field === 'form'}<div class="notice" data-tone="error" role="alert" id="tg-e" tabindex="-1"><Icon name="circle-alert" /><div><p>{tr(formError.message)}</p></div></div>{/if}
{/snippet}

<section class="panel" aria-busy={loading}>
  <header class="panel-head">
    <div class="panel-title"><h2>{tr('navTags')}</h2><p>{tr('lnkTagsPanelHint')}</p></div>
  </header>
  {#if loadError}
    <div class="panel-body">
      <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(loadError)}</p></div><button class="btn" data-size="sm" onclick={() => { loading = true; void load(); }}>{tr('retry')}</button></div>
    </div>
  {/if}
  {#if loading && !app.tags.length}
    <p class="muted tags-loading" role="status">{tr('loading')}</p>
  {:else if !app.tags.length && !loadError}
    <Empty icon="tag" title={tr('noTags')}>{tr('lnkNoTagsHint')}</Empty>
  {:else if app.tags.length}
    <div class="table-wrap">
      <table class="table tags-table">
        <thead><tr><th>{tr('lnkTag')}</th><th class="c-desc">{tr('lnkDescription')}</th><th class="r">{tr('navLinks')}</th><th class="c-actions"><span class="visually-hidden">{tr('lnkActions')}</span></th></tr></thead>
        <tbody>
          {#each app.tags as t (t.name)}
            <tr>
              <td><span class="tag" style:--tag-color={tagColor(t.name)}><i></i>{t.name}</span></td>
              <td class="secondary c-desc"><span class="desc">{t.description ?? ''}</span></td>
              <td class="r num"><a href={href('links', '', { tags: t.name })} aria-label={tr('lnkTagLinks', { name: t.name, count: num(t.link_count) })}>{num(t.link_count)}</a></td>
              <td class="c-actions">
                <div class="row-actions">
                  <button class="btn reveal" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('lnkEditTag', { name: t.name })} use:tip={tr('lnkRenameRecolor')} onclick={() => edit(t)}><Icon name="pencil" /></button>
                  <Menu label={tr('lnkTagActions', { name: t.name })} icon="ellipsis" size="sm">
                    {#snippet children(close)}
                      <button class="menu-item" role="menuitem" onclick={() => { close(); edit(t); }}><Icon name="pencil" />{tr('lnkRenameRecolor')}</button>
                      <button class="menu-item" role="menuitem" disabled={app.tags.length < 2} onclick={() => { close(); formError = null; mergeFrom = t; mergeInto = ''; }}><Icon name="split" />{tr('lnkMergeInto')}</button>
                      <div class="menu-sep"></div>
                      <button class="menu-item" role="menuitem" data-tone="danger" onclick={() => { close(); formError = null; removing = t; }}><Icon name="trash-2" />{tr('lnkRemoveFromAll')}</button>
                    {/snippet}
                  </Menu>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<Dialog open={editing !== null} onclose={() => (editing = null)} title={tr('lnkEditTagTitle')} width={460} {busy}>
  <form id="tag-edit" class="stack" onsubmit={save} novalidate>
    <div class="field">
      <label class="field-label" for="tg-n">{tr('tagName')}</label>
      <input id="tg-n" class="input" bind:value={name} maxlength="40" disabled={busy} aria-invalid={formError?.field === 'name' ? 'true' : undefined} aria-describedby={formError?.field === 'name' ? 'tg-n-e tg-n-h' : 'tg-n-h'} />
      {#if formError?.field === 'name'}<p class="field-error" id="tg-n-e"><Icon name="circle-alert" />{tr(formError.message)}</p>{/if}
      <p class="field-hint" id="tg-n-h">{merging ? tr('lnkMergeWarning', { name: name.trim() }) : tr('lnkRenameHint')}</p>
    </div>
    <div class="field">
      <label class="field-label" for="tg-d">{tr('tagDescription')} <span class="optional">{tr('lnkOptional')}</span></label>
      <textarea id="tg-d" class="textarea" rows="2" maxlength="1000" bind:value={description} disabled={busy} aria-invalid={formError?.field === 'description' ? 'true' : undefined} aria-describedby={formError?.field === 'description' ? 'tg-d-e' : undefined}></textarea>
      {#if formError?.field === 'description'}<p class="field-error" id="tg-d-e"><Icon name="circle-alert" />{tr(formError.message)}</p>{/if}
    </div>
    <fieldset class="fieldset" disabled={busy} aria-describedby={formError?.field === 'color' ? 'tg-c-e' : undefined}>
      <legend>{tr('tagColor')}</legend>
      <div class="swatches" role="radiogroup" aria-label={tr('tagColor')}>
        <label class="sw none" use:tip={tr('lnkNoColor')}><input id="tg-c-none" type="radio" bind:group={color} value="" aria-label={tr('lnkNoColor')} /><Icon name="ban" /></label>
        {#each swatches as c, i (c)}
          <label class="sw" style:--c={c} use:tip={tr('lnkColorN', { n: i + 1 })}><input type="radio" bind:group={color} value={c} aria-label={tr('lnkColorN', { n: i + 1 })} /></label>
        {/each}
        <label class="sw custom" style:--c={custom} use:tip={tr('lnkCustomColor')}>
          <input type="radio" bind:group={color} value={custom} aria-label={tr('lnkCustomColor')} />
        </label>
        <input class="picker" type="color" bind:value={custom} aria-label={tr('lnkPickColor')} oninput={() => (color = custom)} />
      </div>
      {#if formError?.field === 'color'}<p class="field-error" id="tg-c-e"><Icon name="circle-alert" />{tr(formError.message)}</p>{/if}
    </fieldset>
    {@render formNotice()}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={busy} onclick={() => (editing = null)}>{tr('cancel')}</button>
    <button type="submit" form="tag-edit" class="btn" data-variant="primary" disabled={busy} aria-busy={busy}>{#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<span>{merging ? tr('lnkMergeTags') : tr('lnkSaveTag')}</span></button>
  {/snippet}
</Dialog>

<Dialog open={mergeFrom != null} onclose={() => (mergeFrom = null)} title={tr('lnkMergeTitle', { name: mergeFrom?.name ?? '' })} description={tr('lnkMergeDescription', { name: mergeFrom?.name ?? '' })} width={440} {busy}>
  <form id="tag-merge" class="stack" onsubmit={merge}>
    <div class="field">
      <label class="field-label" for="tg-m">{tr('lnkMergeIntoLabel')}</label>
      <select id="tg-m" class="select" bind:value={mergeInto} disabled={busy}>
        <option value="" disabled>{tr('lnkChooseTag')}</option>
        {#each app.tags.filter((t) => t.name !== mergeFrom?.name) as t (t.name)}<option value={t.name}>{t.name}</option>{/each}
      </select>
      <p class="field-hint">{tr('lnkMergeDetailsHint')}</p>
    </div>
    {@render formNotice()}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={busy} onclick={() => (mergeFrom = null)}>{tr('cancel')}</button>
    <button type="submit" form="tag-merge" class="btn" data-variant="primary" disabled={!mergeInto || busy} aria-busy={busy}>{#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<span>{tr('lnkMergeTags')}</span></button>
  {/snippet}
</Dialog>

<Dialog open={removing != null} onclose={() => (removing = null)} title={tr('lnkRemoveTagTitle', { name: removing?.name ?? '' })} width={440} {busy}>
  <form id="tag-remove" class="stack" onsubmit={remove}>
    <p class="secondary confirm-text">{tr('deleteTagHint')} {tr('lnkRemoveTagCount', { count: num(removing?.link_count ?? 0) })}</p>
    {@render formNotice()}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={busy} onclick={() => (removing = null)}>{tr('cancel')}</button>
    <button type="submit" form="tag-remove" class="btn" data-variant="danger" disabled={busy} aria-busy={busy}>{#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="trash-2" /><span>{tr('lnkRemoveTag')}</span></button>
  {/snippet}
</Dialog>

<style>
  .tags-loading {
    padding: var(--space-6) var(--space-4);
  }
  .desc {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-line;
  }
  .confirm-text {
    font-size: var(--text-sm);
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .sw {
    position: relative;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: var(--c);
    cursor: pointer;
  }
  .sw input {
    position: absolute;
    inset: 0;
    appearance: none;
    margin: 0;
    border-radius: inherit;
    cursor: pointer;
  }
  .sw.none {
    border: 1px dashed var(--border);
    background: transparent;
    color: var(--text-muted);
  }
  .sw.none :global(svg) {
    width: 14px;
    height: 14px;
  }
  .sw:has(:checked) {
    box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--text);
  }
  .sw:has(:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 4px;
  }
  .picker {
    width: 40px;
    height: 28px;
    padding: 2px;
    border: 1px solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    cursor: pointer;
  }
  @media (max-width: 640px) {
    .c-desc {
      display: none;
    }
  }
</style>
