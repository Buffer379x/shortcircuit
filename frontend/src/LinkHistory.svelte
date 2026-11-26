<script lang="ts">
  import { onMount } from 'svelte';
  import { linkName } from './api';
  import type { Api, ConditionType, Link, LinkHistory, LinkMetadata, LinkPolicy, LinkRevision } from './api';
  import Dialog from './Dialog.svelte';
  import { fromUnixSeconds, problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  let { t, language, api, link, onclose, onrestore }: {
    t: Messages; language: Language; api: Api; link: Link; onclose: () => void;
    onrestore: (id: number, metadata: LinkMetadata) => void;
  } = $props();

  const actionNames = {
    metadata: 'historyMetadata',
    policy: 'historyPolicy',
    protection: 'historyProtection',
    restore: 'historyRestoreAction',
  } as const satisfies Record<LinkRevision['action'], keyof Messages>;
  const conditionNames = {
    language: 'matchLanguage',
    device: 'matchDevice',
    os: 'matchOs',
    browser: 'matchBrowser',
    query: 'matchQuery',
  } as const satisfies Record<ConditionType, keyof Messages>;

  const path = $derived(`/links/${link.id}/history`);
  const dateFormat = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'medium' }));
  const numberFormat = $derived(new Intl.NumberFormat(language));
  const compareId = $props.id();
  let history = $state.raw<LinkHistory | null>(null);
  let compared = $state<number | null>(null);
  let confirmation = $state.raw<LinkRevision | 'clear' | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);
  const selected = $derived(history?.items.find((entry) => entry.id === compared));
  const differences = $derived(history && selected?.previous ? compare(selected.previous, history.current) : []);

  onMount(() => { void load(); });

  async function load() {
    if (loading) return;
    loading = true;
    error = null;
    try {
      history = await api<LinkHistory>(path);
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      loading = false;
    }
  }

  function date(seconds: number | null): string {
    const timestamp = seconds === null ? null : fromUnixSeconds(seconds);
    return timestamp === null ? t.historyNoValue : dateFormat.format(new Date(timestamp));
  }

  function count(value: number | null): string {
    return value === null ? t.historyNoValue : numberFormat.format(value);
  }

  function status(value: boolean): string {
    return value ? t.enabled : t.disabled;
  }

  function rules(value: LinkPolicy['rules']): string {
    return value.map((rule) => [
      rule.target,
      ...rule.conditions.map(({ type, key, value }) =>
        `${t[conditionNames[type]]} · ${type === 'query' ? `${key} = ` : ''}${value}`),
    ].join('\n')).join('\n\n') || t.historyNoValue;
  }

  function compare(previous: NonNullable<LinkRevision['previous']>, current: LinkHistory['current']) {
    const changes: { label: string; before: string; after: string }[] = [];
    const add = <Value,>(label: string, before: Value, after: Value, render: (value: Value) => string) => {
      if (JSON.stringify(before) !== JSON.stringify(after)) {
        changes.push({ label, before: render(before), after: render(after) });
      }
    };

    if (previous.kind === 'metadata') {
      const old = previous.value;
      const next = current.metadata;
      add(t.target, old.target, next.target, String);
      add(t.historyLinkTitle, old.title, next.title, (value) => value || t.historyNoValue);
      add(t.tags, old.tags, next.tags, (value) => value.join('\n') || t.historyNoValue);
      add(t.status, old.enabled, next.enabled, status);
      add(t.countLinkVisits, old.analytics_enabled, next.analytics_enabled, status);
    } else {
      const old = previous.value;
      const next = current.policy;
      add(t.redirectStatus, old.status, next.status, String);
      add(t.redirectStarts, old.starts_at, next.starts_at, date);
      add(t.redirectExpires, old.expires_at, next.expires_at, date);
      add(t.historyVisitLimit, old.max_visits, next.max_visits, count);
      add(t.forwardQuery, old.forward_query, next.forward_query, status);
      add(t.forwardPath, old.forward_path, next.forward_path, status);
      add(t.redirectRules, old.rules, next.rules, rules);
    }
    return changes;
  }

  function begin(action: LinkRevision | 'clear') {
    error = null;
    notice = null;
    confirmation = action;
  }

  function cancel() {
    confirmation = null;
    error = null;
  }

  async function apply(event: SubmitEvent) {
    event.preventDefault();
    const action = confirmation;
    if (saving || !action) return;

    saving = true;
    error = null;
    try {
      if (action === 'clear') {
        await api<void>(path, 'DELETE');
        notice = 'historyCleared';
      } else {
        await api<void>(`${path}/${action.id}/restore`, 'POST');
        if (action.previous?.kind === 'metadata') onrestore(link.id, action.previous.value);
        notice = 'historyRestored';
      }
      confirmation = null;
      compared = null;
      history = null;
      await load();
    } catch (failure) {
      error = problemMessage(failure);
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

<Dialog {t} title={t.linkHistory} open={true} busy={saving} {onclose}>
  <div class="dialog-body stack" aria-busy={loading || saving}>
    <div>
      <strong>{history ? history.current.metadata.title || `/${link.slug}` : linkName(link)}</strong>
      <p class="muted">{t.historyRetention}</p>
    </div>
    {#if notice}<p class="notice" role="status">{t[notice]}</p>{/if}
    {#if error && !confirmation}
      <p class="notice error" role="alert">{t[error]}</p>
      <div><button class="button" type="button" onclick={load}>{t.retry}</button></div>
    {:else if loading || !history}
      <p role="status">{t.loading}</p>
    {:else if !history.items.length}
      <p class="muted">{t.historyEmpty}</p>
    {:else}
      <div class="row history-actions">
        <button class="button" type="button" onclick={exportHistory}><Icon name="download" />{t.historyExport}</button>
        <button class="button danger" type="button" onclick={() => begin('clear')}>{t.historyClear}</button>
      </div>
      <!-- svelte-ignore a11y_no_redundant_roles (Preserve list semantics when markers are hidden.) -->
      <ol class="revisions" role="list">
        {#each history.items as entry (entry.id)}
          <li>
            <div class="between revision-head">
              <div>
                <strong>{t[actionNames[entry.action]]}</strong>
                <p class="muted revision-meta">
                  <time datetime={fromUnixSeconds(entry.created_at) ?? undefined}>{date(entry.created_at)}</time>
                  <span>{entry.actor.kind === 'api_key' ? `${t.historyApiKey} ${entry.actor.label}` : entry.actor.label}</span>
                </p>
              </div>
              {#if entry.previous}
                <button class="button" type="button" aria-expanded={entry.id === compared}
                  aria-controls={entry.id === compared ? compareId : undefined}
                  onclick={() => compared = entry.id === compared ? null : entry.id}>{t.historyCompare}</button>
              {/if}
            </div>
            {#if entry.id === compared && entry.previous}
              <section id={compareId} aria-labelledby={compareId + '-title'} class="comparison stack">
                <h3 id={compareId + '-title'}>{t.historyCompareHeading}</h3>
                {#each differences as difference}
                  <div class="comparison-field">
                    <h4>{difference.label}</h4>
                    <dl>
                      <div><dt>{t.historyPrevious}</dt><dd>{difference.before}</dd></div>
                      <div><dt>{t.historyCurrent}</dt><dd>{difference.after}</dd></div>
                    </dl>
                  </div>
                {:else}
                  <p>{t.historyCompareEqual}</p>
                {/each}
                {#if differences.length}
                  <div><button class="button" type="button" onclick={() => begin(entry)}>{t.historyRestore}</button></div>
                {/if}
              </section>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
  </div>
</Dialog>

<Dialog {t} title={confirmation === 'clear' ? t.historyClearTitle : t.historyRestoreTitle} open={confirmation !== null} busy={saving} onclose={cancel}>
  {#if confirmation}
    <form onsubmit={apply} aria-busy={saving}>
      <div class="dialog-body stack">
        <p>{confirmation === 'clear' ? t.historyClearHint
          : confirmation.previous?.kind === 'metadata' ? t.historyRestoreMetadataHint : t.historyRestorePolicyHint}</p>
        {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
      </div>
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={saving} onclick={cancel}>{t.cancel}</button>
        <button class="button" class:danger={confirmation === 'clear'} class:primary={confirmation !== 'clear'}
          type="submit" disabled={saving}>{saving ? t.saving : confirmation === 'clear' ? t.historyClear : t.historyRestore}</button>
      </footer>
    </form>
  {/if}
</Dialog>

<style>
  .history-actions, .revision-meta { display: flex; flex-wrap: wrap; gap: 8px; }
  .revisions { margin: 0; padding: 0; list-style: none; border-bottom: 1px solid var(--line); }
  .revisions li { padding-block: 12px; border-top: 1px solid var(--line); }
  .revision-head { align-items: flex-start; gap: 8px; }
  .revision-head > div { min-width: 0; overflow-wrap: anywhere; }
  .revision-head > button { flex-shrink: 0; }
  .revision-meta { font-size: 12px; margin-top: 4px; }
  .comparison { margin-top: 12px; padding: 12px; border-radius: 8px; background: var(--raised); }
  .comparison h3 { margin: 0; font-size: 15px; }
  .comparison h4 { margin: 0 0 4px; font-size: inherit; }
  .comparison dl { margin: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .comparison dt { color: var(--muted); font-size: 12px; }
  .comparison dd { margin: 0; overflow-wrap: anywhere; white-space: pre-wrap; }
  @media (max-width: 420px) {
    .comparison dl { grid-template-columns: minmax(0, 1fr); gap: 4px; }
    .comparison-field + .comparison-field { padding-top: 8px; border-top: 1px solid var(--line); }
  }
</style>
