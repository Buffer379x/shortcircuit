<script lang="ts">
  // Revisions of a link's metadata, redirect settings, and protection: compare with the current
  // settings, restore with confirmation, export as JSON, and clear with confirmation.
  import { tick, untrack } from 'svelte';
  import type { LinkHistory, LinkPolicy, LinkRevision } from '../../api';
  import type { Messages } from '../../i18n';
  import { fromUnixSeconds, problemMessage } from '../../format';
  import Icon from '../../lib/Icon.svelte';
  import Dialog from '../../lib/ui/Dialog.svelte';
  import { app } from '../../lib/app.svelte';
  import { ago, num, when } from '../../lib/fmt';
  import { tr } from '../../lib/locale.svelte';
  import { toast } from '../../lib/toast.svelte';
  import { tip } from '../../lib/tooltip';
  import { describeRule } from './rules';

  let { link, onrestored, oncount }: {
    link: { id: number; host: string; slug: string };
    /** Called after a restore so the page reloads the link and its policy. */
    onrestored: () => void;
    oncount: (count: number) => void;
  } = $props();

  const actionNames = {
    metadata: 'historyMetadata',
    policy: 'historyPolicy',
    protection: 'historyProtection',
    restore: 'historyRestoreAction',
  } as const satisfies Record<LinkRevision['action'], keyof Messages>;

  const uid = $props.id();
  const path = $derived(`/links/${link.id}/history`);
  let history = $state.raw<LinkHistory | null>(null);
  let compared = $state<number | null>(null);
  let confirmation = $state.raw<LinkRevision | 'clear' | null>(null);
  let dialogOpen = $state(false);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<keyof Messages | null>(null);
  let actionError = $state<keyof Messages | null>(null);
  let returnFocus: HTMLElement | null = null;
  const selected = $derived(history?.items.find((entry) => entry.id === compared));
  const differences = $derived(history && selected?.previous ? compare(selected.previous, history.current) : []);

  $effect(() => {
    void link.id;
    untrack(() => {
      history = null;
      compared = null;
      void load();
    });
  });

  async function load() {
    if (loading) return;
    loading = true;
    error = null;
    try {
      history = await app.api<LinkHistory>(path);
      oncount(history.items.length);
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      loading = false;
    }
  }

  const none = () => tr('historyNoValue');
  const date = (seconds: number | null) => (seconds === null || fromUnixSeconds(seconds) === null ? none() : when(seconds));
  const count = (value: number | null) => (value === null ? none() : num(value));
  const status = (value: boolean) => (value ? tr('enabled') : tr('disabled'));
  const rules = (value: LinkPolicy['rules']) =>
    value.map((rule, i) => `${tr('detRuleNumber', { n: i + 1 })}: ${describeRule(rule)} → ${rule.target}`).join('\n') || none();

  function compare(previous: NonNullable<LinkRevision['previous']>, current: LinkHistory['current']) {
    const changes: { label: string; before: string; after: string }[] = [];
    const add = <Value,>(label: string, before: Value, after: Value, render: (value: Value) => string) => {
      if (JSON.stringify(before) !== JSON.stringify(after)) changes.push({ label, before: render(before), after: render(after) });
    };
    if (previous.kind === 'metadata') {
      const old = previous.value;
      const next = current.metadata;
      add(tr('target'), old.target, next.target, String);
      add(tr('historyLinkTitle'), old.title, next.title, (value) => value || none());
      if (old.description !== undefined) add(tr('linkDescription'), old.description, next.description ?? '', (value) => value || none());
      add(tr('tags'), old.tags, next.tags, (value) => value.join(', ') || none());
      add(tr('status'), old.enabled, next.enabled, status);
      add(tr('countLinkVisits'), old.analytics_enabled, next.analytics_enabled, status);
    } else {
      const old = previous.value;
      const next = current.policy;
      add(tr('redirectStatus'), old.status, next.status, String);
      add(tr('redirectStarts'), old.starts_at, next.starts_at, date);
      add(tr('redirectExpires'), old.expires_at, next.expires_at, date);
      add(tr('historyVisitLimit'), old.max_visits, next.max_visits, count);
      add(tr('forwardQuery'), old.forward_query, next.forward_query, status);
      add(tr('forwardPath'), old.forward_path, next.forward_path, status);
      if (old.noindex !== undefined) add(tr('requestNoIndex'), old.noindex, next.noindex ?? false, status);
      add(tr('redirectRules'), old.rules, next.rules, rules);
    }
    return changes;
  }

  function begin(action: LinkRevision | 'clear', event: MouseEvent) {
    returnFocus = event.currentTarget as HTMLElement;
    actionError = null;
    confirmation = action;
    dialogOpen = true;
  }

  function closed() {
    confirmation = null;
    actionError = null;
    returnFocus?.isConnected && returnFocus.focus();
  }

  async function apply() {
    const action = confirmation;
    if (saving || !action) return;
    saving = true;
    actionError = null;
    try {
      if (action === 'clear') {
        await app.api<void>(path, 'DELETE');
        toast(tr('historyCleared'));
      } else {
        await app.api<void>(`${path}/${action.id}/restore`, 'POST');
        toast(tr('historyRestored'), { body: tr(actionNames[action.action]) });
        onrestored();
      }
      returnFocus = null;
      dialogOpen = false;
      compared = null;
      history = null;
      await load();
      await tick();
      document.getElementById(`${uid}-title`)?.focus();
    } catch (failure) {
      actionError = problemMessage(failure);
    } finally {
      saving = false;
    }
  }

  function exportHistory() {
    if (!history?.items.length) return;
    const data = { link: { id: link.id, host: link.host, slug: link.slug }, items: history.items };
    const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2) + '\n'], { type: 'application/json' }));
    const download = document.createElement('a');
    download.href = url;
    download.download = `shortcircuit-link-${link.id}-history.json`;
    download.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
</script>

<section class="panel" aria-labelledby="{uid}-title" aria-busy={loading || saving}>
  <header class="panel-head">
    <div class="panel-title"><h2 id="{uid}-title" tabindex="-1">{tr('linkHistory')}</h2><p>{tr('detHistoryHint')} {tr('historyRetention')}</p></div>
    {#if history?.items.length}
      <span class="spacer"></span>
      <div class="btn-group">
        <button type="button" class="btn" data-size="sm" onclick={exportHistory}><Icon name="download" />{tr('historyExport')}</button>
        <button type="button" class="btn" data-size="sm" data-variant="danger-quiet" onclick={(e) => begin('clear', e)}><Icon name="trash-2" />{tr('historyClear')}</button>
      </div>
    {/if}
  </header>
  {#if error}
    <div class="panel-body stack">
      <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(error)}</p>
      <div><button type="button" class="btn" onclick={load}><Icon name="refresh-cw" />{tr('retry')}</button></div>
    </div>
  {:else if loading || !history}
    <div class="panel-body"><p class="muted" role="status">{tr('loading')}</p></div>
  {:else if !history.items.length}
    <div class="panel-body"><p class="muted">{tr('historyEmpty')}</p></div>
  {:else}
    <!-- svelte-ignore a11y_no_redundant_roles (Keep list semantics when markers are hidden.) -->
    <ol class="timeline" role="list">
      {#each history.items as entry, i (entry.id)}
        <li>
          <span class="tl-dot" class:first={i === 0} aria-hidden="true"></span>
          <div class="tl-body">
            <div class="tl-head">
              <b>{tr(actionNames[entry.action])}</b>
              <span class="muted">
                {entry.actor.kind === 'api_key' ? `${tr('historyApiKey')} ${entry.actor.label}` : entry.actor.label},
                <time datetime={fromUnixSeconds(entry.created_at) ?? undefined} use:tip={when(entry.created_at)}>{ago(entry.created_at)}</time>
              </span>
            </div>
            {#if entry.id === compared && entry.previous}
              <div id="{uid}-cmp-{entry.id}" class="comparison">
                <h3 class="visually-hidden">{tr('historyCompareHeading')}</h3>
                {#each differences as difference (difference.label)}
                  <div class="diff">
                    <span class="diff-label">{difference.label}</span>
                    <del class="mono"><span class="visually-hidden">{tr('historyPrevious')}: </span>{difference.before}</del>
                    <ins class="mono"><span class="visually-hidden">{tr('historyCurrent')}: </span>{difference.after}</ins>
                  </div>
                {:else}
                  <p class="muted small">{tr('historyCompareEqual')}</p>
                {/each}
              </div>
            {/if}
          </div>
          {#if entry.previous}
            <div class="tl-actions btn-group">
              <button type="button" class="btn" data-size="sm" data-variant="ghost" aria-expanded={entry.id === compared}
                aria-controls={entry.id === compared ? `${uid}-cmp-${entry.id}` : undefined}
                onclick={() => (compared = entry.id === compared ? null : entry.id)}>{tr('historyCompare')}</button>
              {#if entry.id === compared && differences.length}
                <button type="button" class="btn" data-size="sm" onclick={(e) => begin(entry, e)}><Icon name="rotate-ccw" />{tr('historyRestore')}</button>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}
</section>

<Dialog bind:open={dialogOpen} title={confirmation === 'clear' ? tr('historyClearTitle') : tr('historyRestoreTitle')} busy={saving} width={440} onclose={closed}>
  {#if confirmation}
    <div class="stack">
      <p>{confirmation === 'clear' ? tr('historyClearHint')
        : confirmation.previous?.kind === 'metadata' ? tr('historyRestoreMetadataHint') : tr('historyRestorePolicyHint')}</p>
      {#if actionError}<p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(actionError)}</p>{/if}
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn" disabled={saving} onclick={() => (dialogOpen = false)}>{tr('cancel')}</button>
    <button type="button" class="btn" data-variant={confirmation === 'clear' ? 'danger' : 'primary'} disabled={saving} onclick={apply}>
      {saving ? tr('saving') : confirmation === 'clear' ? tr('historyClear') : tr('historyRestore')}
    </button>
  {/snippet}
</Dialog>

<style>
  .timeline {
    display: grid;
    margin: 0;
    padding: var(--space-4) var(--space-5);
    list-style: none;
  }
  .timeline li {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    padding-bottom: var(--space-5);
  }
  .timeline li:last-child {
    padding-bottom: 0;
  }
  .timeline li:not(:last-child)::before {
    content: '';
    position: absolute;
    left: 4px;
    top: 14px;
    bottom: 0;
    width: 1px;
    background: var(--border);
  }
  .tl-dot {
    flex: none;
    width: 9px;
    height: 9px;
    margin-top: 5px;
    border-radius: 50%;
    border: 2px solid var(--border-control);
    background: var(--surface);
  }
  .tl-dot.first {
    border-color: var(--accent);
    background: var(--accent);
  }
  .tl-body {
    display: grid;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  .tl-head {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    font-size: var(--text-sm);
  }
  .tl-head .muted,
  .small {
    font-size: var(--text-xs);
  }
  .tl-actions {
    flex: none;
  }
  .comparison {
    display: grid;
    gap: var(--space-3);
  }
  .diff {
    display: grid;
    gap: 2px;
    font-size: var(--text-xs);
  }
  .diff-label {
    color: var(--text-muted);
  }
  .diff del,
  .diff ins {
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    text-decoration: none;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .diff del {
    background: var(--error-soft);
    color: var(--text-secondary);
  }
  .diff del::before {
    content: '− ';
    color: var(--error);
  }
  .diff ins {
    background: var(--success-soft);
    color: var(--text);
  }
  .diff ins::before {
    content: '+ ';
    color: var(--success);
  }
  @media (max-width: 640px) {
    .timeline {
      padding: var(--space-4);
    }
    .timeline li {
      flex-wrap: wrap;
    }
    .tl-body {
      flex-basis: calc(100% - 9px - var(--space-4));
    }
    .tl-actions {
      margin-left: calc(9px + var(--space-4));
    }
  }
</style>
