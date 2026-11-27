<script lang="ts">
  // Edit link: a drawer with the link form. Saving sends only what changed, in separate requests for
  // the link details, redirect settings, protection, and monitoring, so a failure keeps earlier steps.
  import { untrack } from 'svelte';
  import type { CatalogLink, Link, LinkProtection, LinkProtectionChange, Policy } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Drawer from '../lib/ui/Drawer.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Icon from '../lib/Icon.svelte';
  import LinkForm, {
    draftFromPolicy, emptyDraft, errorFor, linkCache, monitoringEnabled, policyOf, readyContext, validateDraft,
    type FormContext, type FormErrors, type LinkDraft,
  } from './LinkForm.svelte';
  import { overlays, linksChanged } from '../lib/overlays.svelte';
  import { app, loadTags } from '../lib/app.svelte';
  import { toast } from '../lib/toast.svelte';
  import { go, route } from '../lib/router.svelte';
  import { tr } from '../lib/locale.svelte';

  let open = $state(false);
  let id = $state<number | null>(null);
  let link = $state<CatalogLink | null>(null);
  let loadError = $state<keyof Messages | null>(null);
  let draft = $state<LinkDraft>(emptyDraft(''));
  let ctx = $state<FormContext>(readyContext());
  let errors = $state<FormErrors>({});
  let policy = $state.raw<Policy | null>(null);
  let protection = $state.raw<LinkProtection | null>(null);
  let monitored = $state<{ enabled: boolean; priority: boolean } | null>(null);
  let confirm = $state(false);
  let busy = $state(false);
  let deleteError = $state<keyof Messages | null>(null);
  let formKey = $state(0);
  let requests: AbortController | null = null;

  $effect(() => {
    const next = overlays.editId;
    if (next == null) return;
    untrack(() => start(next));
  });
  $effect(() => {
    if (!open) untrack(() => {
      overlays.editId = null;
      requests?.abort();
    });
  });

  function start(next: number) {
    requests?.abort();
    requests = new AbortController();
    id = next;
    errors = {};
    loadError = null;
    deleteError = null;
    policy = null;
    protection = null;
    monitored = null;
    ctx = { ...readyContext(), policy: 'loading', protection: 'loading' };
    const cached = linkCache.get(next) ?? null;
    link = null;
    draft = emptyDraft('');
    if (cached) fill(cached);
    open = true;
    void loadLink(next, requests.signal, !cached);
    void loadPolicy(next, requests.signal);
    void loadProtection(next, requests.signal);
  }

  function fill(l: CatalogLink) {
    link = l;
    // Policy and protection may have arrived first; keep their fields.
    Object.assign(draft, {
      target: l.target, host: l.host, slug: l.slug, title: l.title ?? '', description: l.description ?? '', tags: [...l.tags],
      enabled: l.enabled, analytics_enabled: l.analytics_enabled,
    });
    // `monitor` exists only on API versions that report it; without it the current state is unknown.
    monitored = 'monitor' in l ? { enabled: l.monitor != null, priority: l.monitor?.priority ?? false } : null;
    draft.monitor = monitored?.enabled ?? false;
    ctx.ruleCount = l.rule_count ?? ctx.ruleCount;
    void monitoringEnabled().then((on) => (ctx.monitoring = monitored ? on : null));
    formKey++;
  }

  async function loadLink(target: number, signal: AbortSignal, required: boolean) {
    try {
      const fresh = await app.api<CatalogLink>(`/links/${target}`, 'GET', undefined, 'json', signal);
      if (signal.aborted) return;
      linkCache.set(fresh.id, fresh);
      // Keep edits that started from the cached copy; refresh only what the person has not touched.
      if (!link) fill(fresh);
      else link = fresh;
    } catch (error) {
      if (!signal.aborted && required) loadError = problemMessage(error);
    }
  }

  async function loadPolicy(target: number, signal: AbortSignal) {
    ctx.policy = 'loading';
    try {
      const p = await app.api<Policy>(`/links/${target}/policy`, 'GET', undefined, 'json', signal);
      if (signal.aborted) return;
      policy = p;
      draftFromPolicy(draft, p);
      ctx.visitsUsed = p.visits_used;
      ctx.ruleCount = p.rules.length;
      ctx.policy = 'ready';
    } catch {
      if (!signal.aborted) ctx.policy = 'error';
    }
  }

  async function loadProtection(target: number, signal: AbortSignal) {
    ctx.protection = 'loading';
    try {
      const p = await app.api<LinkProtection>(`/links/${target}/protection`, 'GET', undefined, 'json', signal);
      if (signal.aborted) return;
      protection = p;
      draft.access = p.one_time ? 'one_time' : p.password_required ? 'password' : 'none';
      draft.password = '';
      draft.remove_password = false;
      ctx.hasPassword = p.password_required;
      ctx.consumed = p.consumed;
      ctx.protection = 'ready';
    } catch {
      if (!signal.aborted) ctx.protection = 'error';
    }
  }

  function retrySections() {
    if (id == null || !requests) return;
    if (ctx.policy === 'error') void loadPolicy(id, requests.signal);
    if (ctx.protection === 'error') void loadProtection(id, requests.signal);
  }

  function protectionChange(): LinkProtectionChange | null {
    if (!protection || ctx.protection !== 'ready' || protection.consumed) return null;
    const oneTime = draft.access === 'one_time';
    const remove = protection.password_required && (draft.access === 'none' || (oneTime && draft.remove_password));
    const password = draft.access !== 'none' && !remove ? draft.password : '';
    if (oneTime === protection.one_time && !remove && !password) return null;
    return { one_time: oneTime, ...(remove ? { remove_password: true } : password ? { password } : {}) };
  }

  async function save() {
    if (!link || busy) return;
    const found = validateDraft(draft, ctx);
    errors = found;
    if (Object.keys(found).length) return;
    busy = true;
    const target = link.id;
    let changed = false;
    let tagsChanged = false;
    try {
      const d = $state.snapshot(draft);
      const meta = {
        ...(d.target.trim() !== link.target ? { target: d.target.trim() } : {}),
        ...(d.title.trim() !== (link.title ?? '') ? { title: d.title.trim() } : {}),
        ...(d.description.trim() !== (link.description ?? '') ? { description: d.description.trim() } : {}),
        ...(d.tags.join('\n') !== link.tags.join('\n') ? { tags: d.tags } : {}),
        ...(d.enabled !== link.enabled ? { enabled: d.enabled } : {}),
        ...(d.analytics_enabled !== link.analytics_enabled ? { analytics_enabled: d.analytics_enabled } : {}),
      };
      if (Object.keys(meta).length) {
        const updated = await app.api<Link>(`/links/${target}`, 'PATCH', meta);
        tagsChanged = 'tags' in meta;
        link = { ...link, ...updated };
        linkCache.set(target, link);
        changed = true;
      }

      // A protected link needs a temporary redirect: lift protection before switching to a permanent
      // status, and switch to a temporary status before adding protection.
      const savePolicy = async () => {
        if (!policy || ctx.policy !== 'ready') return;
        const next = policyOf(d, policy.rules);
        const { visits_used: _used, ...current } = policy;
        if (JSON.stringify(next) === JSON.stringify({ ...current, noindex: current.noindex ?? false })) return;
        const saved = await app.api<Policy>(`/links/${target}/policy`, 'PATCH', next);
        policy = saved ?? { ...next, visits_used: policy.visits_used };
        changed = true;
      };
      const saveProtection = async () => {
        const change = protectionChange();
        if (!change) return;
        const saved = await app.api<LinkProtection>(`/links/${target}/protection`, 'PATCH', change);
        protection = saved ?? { ...protection!, one_time: change.one_time, password_required: change.remove_password ? false : !!change.password || protection!.password_required };
        ctx.hasPassword = protection.password_required;
        draft.password = '';
        draft.remove_password = false;
        changed = true;
      };
      if (d.redirect_status === 301 || d.redirect_status === 308) {
        await saveProtection();
        await savePolicy();
      } else {
        await savePolicy();
        await saveProtection();
      }

      if (monitored && ctx.monitoring && d.monitor !== monitored.enabled) {
        await app.api(`/monitoring/links/${target}`, 'PATCH', { enabled: d.monitor, priority: monitored.priority });
        monitored = { ...monitored, enabled: d.monitor };
        changed = true;
      }
      toast(tr('linkUpdated'), { body: `${link.host}/${link.slug}` });
      open = false;
    } catch (error) {
      errors = errorFor(problemMessage(error));
    } finally {
      busy = false;
      if (changed) {
        linksChanged();
        if (tagsChanged) void loadTags();
      }
    }
  }

  async function remove(ev: SubmitEvent) {
    ev.preventDefault();
    if (!link || busy) return;
    busy = true;
    deleteError = null;
    const name = `${link.host}/${link.slug}`;
    try {
      await app.api<void>(`/links/${link.id}`, 'DELETE');
      linkCache.delete(link.id);
      confirm = false;
      open = false;
      toast(tr('linkDeleted'), { body: name });
      linksChanged();
      void loadTags();
      if (app.counts.links != null) app.counts.links = Math.max(0, app.counts.links - 1);
      if (route.page === 'links' && route.id) go('links');
      // The row that opened the drawer is gone, so continue from the page heading.
      requestAnimationFrame(() => document.querySelector<HTMLElement>('main h1')?.focus());
    } catch (error) {
      deleteError = problemMessage(error);
    } finally {
      busy = false;
    }
  }
</script>

<Drawer bind:open title={tr('editLink')} description={link ? `${link.host}/${link.slug}` : ''} width={580}>
  {#if loadError && !link}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr(loadError)}</p></div>
      <button type="button" class="btn" data-size="sm" onclick={() => id != null && start(id)}>{tr('retry')}</button>
    </div>
  {:else if !link}
    <p class="muted" role="status">{tr('loading')}</p>
  {:else}
    <form id="edit-link" class="edit-form" novalidate aria-busy={busy} onsubmit={(e) => { e.preventDefault(); void save(); }}>
      <label class="check status-row">
        <input class="switch" type="checkbox" bind:checked={draft.enabled} disabled={busy} />
        <span>{draft.enabled ? tr('enabled') : tr('disabled')}<small>{draft.enabled ? tr('lnkEnabledHint') : tr('lnkDisabledHint')}</small></span>
      </label>
      {#key formKey}
        <LinkForm bind:draft {errors} mode="edit" linkId={link.id} {ctx} {busy} onretry={retrySections} />
      {/key}
    </form>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn" data-variant="danger-quiet" disabled={!link || busy} onclick={() => { deleteError = null; confirm = true; }}><Icon name="trash-2" />{tr('delete')}</button>
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" onclick={() => (open = false)}>{tr('cancel')}</button>
    <button type="submit" form="edit-link" class="btn" data-variant="primary" disabled={!link || busy} aria-busy={busy}>
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<span>{tr('save')}</span>
    </button>
  {/snippet}
</Drawer>

<Dialog bind:open={confirm} title={tr('lnkDeleteOneTitle')} width={440} {busy}>
  <form id="delete-link" onsubmit={remove}>
    <p class="secondary confirm-text">
      <code>{link?.host}/{link?.slug}</code> {tr('lnkDeleteOneHint')}
    </p>
    {#if deleteError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(deleteError)}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button type="button" class="btn" data-variant="ghost" disabled={busy} onclick={() => (confirm = false)}>{tr('cancel')}</button>
    <button type="submit" form="delete-link" class="btn" data-variant="danger" disabled={busy} aria-busy={busy}>
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="trash-2" /><span>{tr('deleteLink')}</span>
    </button>
  {/snippet}
</Dialog>

<style>
  .edit-form {
    display: grid;
    gap: var(--space-5);
  }
  .status-row {
    padding: 12px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
  }
  .confirm-text {
    font-size: var(--text-sm);
  }
</style>
