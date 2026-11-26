<script lang="ts">
  import { shortUrl } from './api';
  import type { Api, Domains, Link } from './api';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Messages } from './i18n';

  type Item = Pick<Link, 'id' | 'host' | 'slug' | 'title' | 'enabled'>;
  type Results = { items: Item[]; has_more: boolean };

  let { t, api, domains, value, excludeId }: {
    t: Messages;
    api: Api;
    domains: Domains;
    value: string;
    excludeId?: number;
  } = $props();

  let requested = $state('');
  let results = $state<Results>();
  let busy = $state(false);
  let notice = $state<keyof Messages | null>(null);
  let error = $state<keyof Messages | null>(null);
  const target = $derived(value.trim());

  async function lookup() {
    if (!target || busy) return;
    const candidate = target;
    requested = candidate;
    busy = true;
    results = undefined;
    error = null;
    notice = null;
    try {
      results = await api<Results>('/links/duplicates', 'POST', {
        target: candidate,
        ...(excludeId === undefined ? {} : { exclude_id: excludeId }),
      });
    } catch (result) {
      error = problemMessage(result);
    } finally {
      busy = false;
    }
  }

  async function copy(item: Item) {
    notice = null;
    error = null;
    try {
      await navigator.clipboard.writeText(shortUrl(item, domains));
      notice = 'copiedUrl';
    } catch {
      error = 'copyFailed';
    }
  }
</script>

{#if target}
  <div class="stack compact">
    <div><button type="button" class="button" disabled={busy} onclick={lookup}><Icon name="search" />{busy ? t.loading : t.findDuplicates}</button></div>
    {#if requested === target}
      {#if results?.items.length}
        <ul class="match-list stack compact" aria-label={t.duplicateResults}>
          {#each results.items as item (item.id)}
            <li class="between">
              <div class="match-label">
                {#if item.title}<strong>{item.title}</strong>{/if}
                <div class="mono">{item.host}/{item.slug}</div>
                {#if !item.enabled}<span class="muted">{t.disabled}</span>{/if}
              </div>
              <button type="button" class="icon-button ghost" aria-label={t.copyUrl} title={t.copyUrl} onclick={() => copy(item)}><Icon name="copy" /></button>
            </li>
          {/each}
        </ul>
        {#if results.has_more}<p>{t.moreDuplicates}</p>{/if}
      {:else if results}
        <p role="status">{t.noDuplicates}</p>
      {/if}
      {#if notice}<p role="status">{t[notice]}</p>{/if}
      {#if error}<p role="alert" class="notice error">{t[error]}</p>{/if}
    {/if}
  </div>
{/if}

<style>
  .match-list { list-style: none; margin: 0; padding: 0; }
  li { padding: 4px 8px; border: 1px solid var(--line); border-radius: 8px; }
  .match-label { min-inline-size: 0; overflow-wrap: anywhere; }
</style>
