<script lang="ts" module>
  import type { CatalogLink, LinkPolicy, Policy } from '../api';
  import { passwordAllowed } from '../format';
  import type { Messages } from '../i18n';
  import { app } from '../lib/app.svelte';

  export type Access = 'none' | 'password' | 'one_time';
  export type Field = 'target' | 'slug' | 'tags' | 'title' | 'description' | 'starts' | 'expires' | 'max' | 'status' | 'access' | 'password' | 'form';
  export type FormErrors = Partial<Record<Field, keyof Messages>>;

  export interface LinkDraft {
    target: string;
    host: string;
    slug: string;
    title: string;
    description: string;
    tags: string[];
    enabled: boolean;
    analytics_enabled: boolean;
    starts_at: string;
    expires_at: string;
    max_visits: string;
    redirect_status: number;
    forward_query: boolean;
    forward_path: boolean;
    noindex: boolean;
    access: Access;
    password: string;
    remove_password: boolean;
    monitor: boolean;
  }

  /** What the form knows about the saved link: existing protection, counters, and loading state. */
  export interface FormContext {
    hasPassword: boolean;
    consumed: boolean;
    visitsUsed: number;
    ruleCount: number;
    policy: 'loading' | 'ready' | 'error';
    protection: 'loading' | 'ready' | 'error';
    /** Whether link monitoring is enabled on the server; null hides the option. */
    monitoring: boolean | null;
  }

  export const emptyDraft = (host: string): LinkDraft => ({
    target: '', host, slug: '', title: '', description: '', tags: [], enabled: true, analytics_enabled: true,
    starts_at: '', expires_at: '', max_visits: '', redirect_status: 302, forward_query: false, forward_path: false,
    noindex: false, access: 'none', password: '', remove_password: false, monitor: false,
  });

  export const readyContext = (): FormContext => ({
    hasPassword: false, consumed: false, visitsUsed: 0, ruleCount: 0, policy: 'ready', protection: 'ready', monitoring: null,
  });

  /** Links seen in lists, so overlays can open before GET /links/{id} answers or where it is unavailable. */
  export const linkCache = new Map<number, CatalogLink>();

  /** A link to copy into the create dialog (Duplicate). Read once when the dialog opens. */
  export const createSeed = $state<{ link: CatalogLink | null }>({ link: null });

  let monitoringCheck: Promise<boolean | null> | null = null;
  /** Reads once per session whether outbound link monitoring is enabled on the server. */
  export function monitoringEnabled(): Promise<boolean | null> {
    monitoringCheck ??= app
      .api<{ monitoring?: { enabled?: boolean } }>('/overview')
      .then((o) => o.monitoring?.enabled ?? null)
      .catch(() => {
        monitoringCheck = null;
        return null;
      });
    return monitoringCheck;
  }

  const SLUG_CHARS = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  export function generateSlug() {
    const bytes = crypto.getRandomValues(new Uint8Array(8));
    return [...bytes].map((b) => SLUG_CHARS[b % SLUG_CHARS.length]).join('');
  }

  /** Unix seconds to the value of a datetime-local input in the browser time zone. */
  export function asLocal(seconds: number | null | undefined): string {
    if (seconds == null) return '';
    const date = new Date(seconds * 1000);
    if (Number.isNaN(date.getTime())) return '';
    return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 19);
  }

  export function asTimestamp(value: string): number | null {
    return value ? Math.floor(new Date(value).getTime() / 1000) : null;
  }

  export function policyOf(d: LinkDraft, rules: Policy['rules']): LinkPolicy {
    return {
      status: d.redirect_status,
      starts_at: asTimestamp(d.starts_at),
      expires_at: asTimestamp(d.expires_at),
      max_visits: d.max_visits.trim() ? Number(d.max_visits) : null,
      forward_query: d.forward_query,
      forward_path: d.forward_path,
      noindex: d.noindex,
      rules,
    };
  }

  export function draftFromPolicy(d: LinkDraft, p: Policy) {
    d.redirect_status = p.status;
    d.starts_at = asLocal(p.starts_at);
    d.expires_at = asLocal(p.expires_at);
    d.max_visits = p.max_visits == null ? '' : String(p.max_visits);
    d.forward_query = p.forward_query;
    d.forward_path = p.forward_path;
    d.noindex = p.noindex ?? false;
  }

  // Server problems that belong to one field are shown next to it and move focus there.
  const FIELD_OF: Partial<Record<keyof Messages, Field>> = {
    invalidTarget: 'target', destinationNotAllowed: 'target', invalidTitle: 'title', invalidDescription: 'description',
    invalidSlug: 'slug', slugInUse: 'slug', invalidHost: 'slug', invalidTags: 'tags', invalidRedirectWindow: 'expires',
    invalidVisitLimit: 'max', invalidRedirectStatus: 'status', permanentRestriction: 'status',
    protectionRequiresTemporary: 'status', invalidPassword: 'password', oneTimeConsumed: 'access',
  };
  export function errorFor(message: keyof Messages): FormErrors {
    return { [FIELD_OF[message] ?? 'form']: message };
  }

  export function validateDraft(d: LinkDraft, ctx: FormContext): FormErrors {
    const e: FormErrors = {};
    try {
      const url = new URL(d.target.trim());
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error();
    } catch {
      e.target = 'invalidTarget';
    }
    if ([...d.description.trim()].length > 500) e.description = 'invalidDescription';
    if (ctx.policy === 'ready') {
      const start = asTimestamp(d.starts_at);
      const end = asTimestamp(d.expires_at);
      if ((start !== null && !Number.isFinite(start)) || (end !== null && !Number.isFinite(end))) e.expires = 'invalidRedirectWindow';
      else if (start !== null && end !== null && end <= start) e.expires = 'invalidRedirectWindow';
      const max = d.max_visits.trim();
      if (max && (!/^\d+$/.test(max) || Number(max) < 1 || Number(max) > Number.MAX_SAFE_INTEGER)) e.max = 'invalidVisitLimit';
      const permanent = d.redirect_status === 301 || d.redirect_status === 308;
      if (permanent && d.access !== 'none') e.status = 'protectionRequiresTemporary';
      else if (permanent && (start !== null || end !== null || max || ctx.ruleCount > 0)) e.status = 'permanentRestriction';
    }
    if (ctx.protection === 'ready' && !ctx.consumed && d.access !== 'none') {
      const needed = d.access === 'password' && !ctx.hasPassword;
      if ((needed && !d.password) || (d.password && !passwordAllowed(d.password))) e.password = 'invalidPassword';
    }
    return e;
  }
</script>

<script lang="ts">
  // The link form shared by New link (dialog) and Edit link (drawer). The essentials come first;
  // everything else sits in sections that open on demand and show a summary when closed.
  import { tick, untrack } from 'svelte';
  import type { Link } from '../api';
  import { problemMessage } from '../format';
  import { proposeCleanUrl } from '../url-cleaner';
  import Icon from '../lib/Icon.svelte';
  import TagInput from '../lib/ui/TagInput.svelte';
  import ShortUrl from '../lib/ui/ShortUrl.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import { shortUrl } from '../lib/app.svelte';
  import { date } from '../lib/fmt';
  import { href } from '../lib/router.svelte';
  import { locale, tr } from '../lib/locale.svelte';
  import { tip } from '../lib/tooltip';

  let {
    draft = $bindable(),
    errors = {},
    mode,
    linkId,
    ctx,
    busy = false,
    onretry,
  }: {
    draft: LinkDraft;
    errors?: FormErrors;
    mode: 'create' | 'edit';
    linkId?: number;
    ctx: FormContext;
    busy?: boolean;
    onretry?: () => void;
  } = $props();

  const ORDER: Field[] = ['target', 'slug', 'tags', 'title', 'description', 'starts', 'expires', 'max', 'access', 'password', 'status'];
  const SECTION: Partial<Record<Field, 'meta' | 'schedule' | 'access' | 'redirect'>> = {
    title: 'meta', description: 'meta', starts: 'schedule', expires: 'schedule', max: 'schedule',
    access: 'access', password: 'access', status: 'redirect',
  };
  const FOCUS: Record<Field, string> = {
    target: 'lf-target', slug: 'lf-slug', tags: 'lf-tags', title: 'lf-title', description: 'lf-desc', starts: 'lf-start',
    expires: 'lf-end', max: 'lf-max', access: 'lf-access-none', password: 'lf-pw', status: 'lf-status', form: 'lf-form-e',
  };
  // The form is keyed per link, so the initial mode decides which section starts open.
  const open = $state({ meta: untrack(() => mode === 'edit'), schedule: false, access: false, redirect: false, tracking: false });

  // Open the section that holds the first error and move focus to the field.
  $effect(() => {
    const first = ORDER.find((f) => errors[f]) ?? (errors.form ? 'form' : undefined);
    if (!first) return;
    const section = SECTION[first];
    if (section) open[section] = true;
    void tick().then(() => document.getElementById(FOCUS[first])?.focus());
  });

  const desc = (field: Field, hint?: string) => [errors[field] ? `lf-${field}-e` : '', hint ?? ''].filter(Boolean).join(' ') || undefined;
  const hosts = $derived.by(() => {
    const list = app.domains?.hosts ?? [];
    return draft.host && !list.includes(draft.host) ? [draft.host, ...list] : list;
  });

  // URL cleaner: tracking parameters with the default list, or a custom list and exceptions.
  let cleanerOpen = $state(false);
  let cleanerNames = $state('');
  let cleanerExceptions = $state('');
  const list = (value: string) => new Set(value.split(',').map((v) => v.trim().toLowerCase()).filter(Boolean));
  const source = $derived(draft.target.trim());
  const quick = $derived(proposeCleanUrl(source));
  const custom = $derived(proposeCleanUrl(source, cleanerNames.trim() ? list(cleanerNames) : undefined, list(cleanerExceptions)));
  const hasQuery = $derived(/^https?:\/\/[^?#]+\?/i.test(source));

  function applyClean(cleaned: string) {
    draft.target = cleaned;
    cleanerOpen = false;
  }

  // Links that already use this exact destination.
  type Match = Pick<Link, 'id' | 'host' | 'slug' | 'title' | 'enabled'>;
  let matches = $state<{ target: string; items: Match[]; more: boolean } | null>(null);
  let matchError = $state<keyof Messages | null>(null);
  $effect(() => {
    const target = source;
    matchError = null;
    if (!/^https?:\/\/[^/\s]+\.[^/\s]+/i.test(target)) {
      matches = null;
      return;
    }
    const request = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const result = await app.api<{ items: Match[]; has_more: boolean }>(
          '/links/duplicates', 'POST', { target, ...(linkId === undefined ? {} : { exclude_id: linkId }) }, 'json', request.signal,
        );
        if (!request.signal.aborted) matches = { target, items: result.items, more: result.has_more };
      } catch (error) {
        if (!request.signal.aborted) {
          matches = null;
          matchError = problemMessage(error);
        }
      }
    }, 450);
    return () => {
      clearTimeout(timer);
      request.abort();
    };
  });
  const duplicates = $derived(matches && matches.target === source ? matches.items : []);

  function newSlug() {
    draft.slug = generateSlug();
  }

  const loadingText = tr('loading');
  const scheduleSummary = $derived.by(() => {
    if (ctx.policy !== 'ready') return ctx.policy === 'loading' ? loadingText : tr('lnkNotLoaded');
    const parts = [
      draft.starts_at && tr('lnkStarts', { date: safeDate(draft.starts_at) }),
      draft.expires_at && tr('lnkEnds', { date: safeDate(draft.expires_at) }),
      draft.max_visits.trim() && tr('lnkVisitCount', { count: Number(draft.max_visits) ? new Intl.NumberFormat(locale.language).format(Number(draft.max_visits)) : draft.max_visits }),
    ].filter(Boolean);
    return parts.join(', ') || tr('lnkAlwaysActive');
  });
  const accessSummary = $derived.by(() => {
    if (ctx.protection !== 'ready') return ctx.protection === 'loading' ? loadingText : tr('lnkNotLoaded');
    if (ctx.consumed) return tr('lnkConsumed');
    if (draft.access === 'password') return tr('protectionPassword');
    if (draft.access === 'one_time') return draft.password || (ctx.hasPassword && !draft.remove_password) ? tr('lnkOneTimePassword') : tr('lnkOneTimeLink');
    return tr('lnkPublic');
  });
  const redirectSummary = $derived.by(() => {
    if (ctx.policy !== 'ready') return ctx.policy === 'loading' ? loadingText : tr('lnkNotLoaded');
    return [String(draft.redirect_status), draft.forward_query && tr('lnkForwardsQuery'), draft.forward_path && tr('lnkForwardsPath'), draft.noindex && tr('lnkNoindexShort')]
      .filter(Boolean).join(', ');
  });
  const trackingSummary = $derived(
    [draft.analytics_enabled ? tr('lnkAnalyticsOn') : tr('lnkAnalyticsOff'), ctx.monitoring && draft.monitor ? tr('lnkMonitored') : '']
      .filter(Boolean).join(', '),
  );

  function safeDate(value: string) {
    const t = new Date(value).getTime();
    return Number.isNaN(t) ? value : date(t / 1000);
  }
</script>

{#snippet error(field: Field)}
  {#if errors[field]}<p class="field-error" id="lf-{field}-e"><Icon name="circle-alert" />{tr(errors[field])}</p>{/if}
{/snippet}

{#snippet sectionState(state: 'loading' | 'ready' | 'error')}
  {#if state === 'loading'}
    <p class="muted" role="status">{tr('loading')}</p>
  {:else}
    <div class="notice" data-tone="error" role="alert">
      <Icon name="circle-alert" /><div><p>{tr('lnkSectionFailed')}</p></div>
      {#if onretry}<button type="button" class="btn" data-size="sm" onclick={onretry}>{tr('retry')}</button>{/if}
    </div>
  {/if}
{/snippet}

<fieldset class="form-fields" disabled={busy}>
  <legend class="visually-hidden">{mode === 'create' ? tr('newLink') : tr('editLink')}</legend>

  <div class="field">
    <label class="field-label" for="lf-target">{tr('target')}</label>
    <input
      id="lf-target"
      class="input"
      type="url"
      inputmode="url"
      placeholder={tr('lnkTargetPlaceholder')}
      autocomplete="off"
      spellcheck="false"
      maxlength="4096"
      bind:value={draft.target}
      aria-invalid={errors.target ? 'true' : undefined}
      aria-describedby={desc('target')}
    />
    {@render error('target')}
    {#if quick.status === 'changes' && !cleanerOpen}
      <div class="inline-hint">
        <Icon name="brush-cleaning" />
        <span>{tr('lnkTrackers', { count: quick.removed.length })} <code>{quick.removed.join(', ')}</code></span>
        <button type="button" class="btn" data-variant="link" data-size="sm" onclick={() => quick.status === 'changes' && applyClean(quick.cleaned)}>{tr('lnkRemove')}</button>
        <button type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('cleanerOptions')} use:tip={tr('cleanerOptions')} aria-expanded={cleanerOpen} onclick={() => (cleanerOpen = true)}>
          <Icon name="sliders-horizontal" />
        </button>
      </div>
    {:else if hasQuery && !cleanerOpen}
      <div><button type="button" class="btn" data-variant="link" data-size="sm" aria-expanded="false" onclick={() => (cleanerOpen = true)}><Icon name="brush-cleaning" />{tr('urlCleaner')}</button></div>
    {/if}
    {#if cleanerOpen}
      <section class="cleaner" aria-label={tr('urlCleaner')}>
        <div class="cleaner-head">
          <b>{tr('urlCleaner')}</b>
          <button type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('close')} use:tip={tr('close')} onclick={() => (cleanerOpen = false)}><Icon name="x" /></button>
        </div>
        <div class="row-2">
          <div class="field">
            <label class="field-label" for="lf-cn">{tr('cleanerNames')} <span class="optional">{tr('lnkOptional')}</span></label>
            <input id="lf-cn" class="input" data-size="sm" maxlength="512" bind:value={cleanerNames} aria-describedby="lf-cn-h" />
            <p class="field-hint" id="lf-cn-h">{tr('cleanerNamesHint')}</p>
          </div>
          <div class="field">
            <label class="field-label" for="lf-ce">{tr('cleanerExceptions')} <span class="optional">{tr('lnkOptional')}</span></label>
            <input id="lf-ce" class="input" data-size="sm" maxlength="512" bind:value={cleanerExceptions} />
          </div>
        </div>
        {#if custom.status === 'changes'}
          <dl class="facts">
            <dt>{tr('cleanerOriginal')}</dt><dd class="mono wrap">{source}</dd>
            <dt>{tr('cleanerPreview')}</dt><dd class="mono wrap">{custom.cleaned}</dd>
            <dt>{tr('cleanerRemoved')}</dt><dd>{custom.removed.join(', ')}</dd>
          </dl>
          <div><button type="button" class="btn" data-size="sm" onclick={() => custom.status === 'changes' && applyClean(custom.cleaned)}>{tr('cleanerApply')}</button></div>
        {:else}
          <p class="field-hint" role="status">{tr(custom.status === 'protected' ? 'cleanerProtected' : custom.status === 'invalid' ? 'invalidTarget' : 'cleanerUnchanged')}</p>
        {/if}
      </section>
    {/if}
    {#each duplicates as d (d.id)}
      <div class="inline-hint" data-tone="info">
        <Icon name="copy-plus" />
        <span>{tr('lnkAlreadyShortened')} <a href={href('links', d.id)}><ShortUrl host={d.host} slug={d.slug} /></a>{#if !d.enabled} · {tr('disabled')}{/if}</span>
        <CopyButton value={shortUrl(d)} label={tr('lnkCopyExisting')} />
      </div>
    {/each}
    {#if matches?.more && duplicates.length}<p class="field-hint">{tr('moreDuplicates')}</p>{/if}
    {#if matchError}<p class="field-hint">{tr('lnkDuplicateCheckFailed')}</p>{/if}
  </div>

  <div class="field">
    <label class="field-label" for="lf-slug">{tr('shortLink')}</label>
    <div class="input-group">
      <select class="select" aria-label={tr('domain')} bind:value={draft.host} disabled={mode === 'edit' || busy}>
        {#each hosts as h (h)}<option value={h}>{h}/</option>{/each}
      </select>
      <input
        id="lf-slug"
        class="input mono"
        placeholder={mode === 'create' ? tr('lnkSlugPlaceholder') : ''}
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        maxlength="200"
        bind:value={draft.slug}
        readonly={mode === 'edit'}
        aria-invalid={errors.slug ? 'true' : undefined}
        aria-describedby={desc('slug', 'lf-slug-h')}
      />
      {#if mode === 'create'}
        <button type="button" class="btn" data-variant="ghost" data-icon-only aria-label={tr('lnkGenerateSlug')} use:tip={tr('lnkGenerateSlug')} onclick={newSlug}>
          <Icon name="refresh-cw" />
        </button>
      {/if}
    </div>
    {@render error('slug')}
    <p class="field-hint" id="lf-slug-h">{mode === 'create' ? tr('lnkSlugHint') : tr('lnkSlugFixed')}</p>
  </div>

  <div class="field">
    <label class="field-label" for="lf-tags">{tr('tags')} <span class="optional">{tr('lnkOptional')}</span></label>
    <TagInput id="lf-tags" bind:value={draft.tags} describedby={desc('tags', 'lf-tags-h')} />
    {@render error('tags')}
    <p class="visually-hidden" id="lf-tags-h">{tr('lnkTagsHint')}</p>
  </div>

  <div class="sections">
    <details class="disclosure" bind:open={open.meta}>
      <summary><Icon name="file-text" />{tr('lnkSectionMeta')}<span class="meta">{draft.title.trim() ? tr('lnkCustomTitle') : tr('lnkNoTitle')}</span><Icon name="chevron-right" /></summary>
      <div class="disclosure-body">
        <div class="field">
          <label class="field-label" for="lf-title">{tr('historyLinkTitle')} <span class="optional">{tr('lnkOptional')}</span></label>
          <input id="lf-title" class="input" bind:value={draft.title} maxlength="160" aria-invalid={errors.title ? 'true' : undefined} aria-describedby={desc('title', 'lf-title-h')} />
          {@render error('title')}
          <p class="field-hint" id="lf-title-h">{tr('lnkTitleHint')}</p>
        </div>
        <div class="field">
          <label class="field-label" for="lf-desc">{tr('lnkDescription')} <span class="optional">{tr('lnkOptional')}</span></label>
          <textarea id="lf-desc" class="textarea" bind:value={draft.description} rows="2" maxlength="1000" placeholder={tr('lnkDescriptionPlaceholder')} aria-invalid={errors.description ? 'true' : undefined} aria-describedby={desc('description')}></textarea>
          {@render error('description')}
        </div>
      </div>
    </details>

    <details class="disclosure" bind:open={open.schedule}>
      <summary><Icon name="calendar-clock" />{tr('lnkSectionSchedule')}<span class="meta">{scheduleSummary}</span><Icon name="chevron-right" /></summary>
      <div class="disclosure-body">
        {#if ctx.policy !== 'ready'}
          {@render sectionState(ctx.policy)}
        {:else}
          <div class="row-2">
            <div class="field">
              <label class="field-label" for="lf-start">{tr('lnkStartsLabel')} <span class="optional">{tr('lnkOptional')}</span></label>
              <input id="lf-start" class="input" type="datetime-local" step="1" bind:value={draft.starts_at} aria-invalid={errors.starts ? 'true' : undefined} aria-describedby={desc('starts')} />
              {@render error('starts')}
            </div>
            <div class="field">
              <label class="field-label" for="lf-end">{tr('lnkEndsLabel')} <span class="optional">{tr('lnkOptional')}</span></label>
              <input id="lf-end" class="input" type="datetime-local" step="1" bind:value={draft.expires_at} aria-invalid={errors.expires ? 'true' : undefined} aria-describedby={desc('expires')} />
              {@render error('expires')}
            </div>
          </div>
          <p class="field-hint">{tr('lnkLocalTime')}</p>
          <div class="field">
            <label class="field-label" for="lf-max">{tr('lnkVisitLimit')} <span class="optional">{tr('lnkOptional')}</span></label>
            <input id="lf-max" class="input narrow" type="number" min="1" max={Number.MAX_SAFE_INTEGER} step="1" inputmode="numeric" value={draft.max_visits} oninput={(e) => (draft.max_visits = e.currentTarget.value)} placeholder={tr('lnkNoLimit')} aria-invalid={errors.max ? 'true' : undefined} aria-describedby={desc('max', 'lf-max-h')} />
            {@render error('max')}
            <p class="field-hint" id="lf-max-h">
              {tr('lnkVisitLimitHint')}
              {#if mode === 'edit' && draft.max_visits.trim()}{tr('lnkVisitsUsed', { count: new Intl.NumberFormat(locale.language).format(ctx.visitsUsed) })}{/if}
            </p>
          </div>
        {/if}
      </div>
    </details>

    <details class="disclosure" bind:open={open.access}>
      <summary><Icon name="lock" />{tr('lnkSectionAccess')}<span class="meta">{accessSummary}</span><Icon name="chevron-right" /></summary>
      <div class="disclosure-body">
        {#if ctx.protection !== 'ready'}
          {@render sectionState(ctx.protection)}
        {:else if ctx.consumed}
          <div class="notice" id="lf-access-none" tabindex="-1"><Icon name="info" /><div><p>{tr('oneTimeConsumed')}</p></div></div>
          {@render error('access')}
        {:else}
          <div class="choice-grid" role="radiogroup" aria-label={tr('lnkSectionAccess')}>
            <label class="choice"><input id="lf-access-none" class="radio" type="radio" bind:group={draft.access} value="none" /><span><b>{tr('lnkPublic')}</b><small>{tr('lnkPublicHint')}</small></span></label>
            <label class="choice"><input class="radio" type="radio" bind:group={draft.access} value="password" /><span><b>{tr('protectionPassword')}</b><small>{tr('lnkPasswordHint')}</small></span></label>
            <label class="choice"><input class="radio" type="radio" bind:group={draft.access} value="one_time" /><span><b>{tr('protectionOneTime')}</b><small>{tr('oneTimeLinkHint')}</small></span></label>
          </div>
          {@render error('access')}
          {#if ctx.hasPassword}
            <p class="field-hint">{draft.access === 'none' ? tr('lnkPasswordWillBeRemoved') : tr('linkPasswordEnabled')}</p>
          {/if}
          {#if draft.access !== 'none'}
            <div class="field">
              <label class="field-label" for="lf-pw">
                {ctx.hasPassword ? tr('newPassword') : tr('password')}
                {#if ctx.hasPassword || draft.access === 'one_time'}<span class="optional">{tr('lnkOptional')}</span>{/if}
              </label>
              <input id="lf-pw" class="input" type="password" autocomplete="new-password" bind:value={draft.password} disabled={draft.remove_password} aria-invalid={errors.password ? 'true' : undefined} aria-describedby={desc('password', 'lf-pw-h')} />
              {@render error('password')}
              <p class="field-hint" id="lf-pw-h">{#if ctx.hasPassword}{tr('keepLinkPassword')} {/if}{tr('passwordHint')} {tr('lnkProtectedHint')}</p>
            </div>
            {#if ctx.hasPassword && draft.access === 'one_time'}
              <label class="check"><input class="checkbox" type="checkbox" bind:checked={draft.remove_password} onchange={() => { if (draft.remove_password) draft.password = ''; }} /><span>{tr('removeLinkPassword')}</span></label>
            {/if}
          {/if}
        {/if}
      </div>
    </details>

    <details class="disclosure" bind:open={open.redirect}>
      <summary><Icon name="split" />{tr('lnkSectionRedirect')}<span class="meta">{redirectSummary}</span><Icon name="chevron-right" /></summary>
      <div class="disclosure-body">
        {#if ctx.policy !== 'ready'}
          {@render sectionState(ctx.policy)}
        {:else}
          <div class="field">
            <label class="field-label" for="lf-status">{tr('redirectStatus')}</label>
            <select id="lf-status" class="select narrow-select" bind:value={draft.redirect_status} aria-invalid={errors.status ? 'true' : undefined} aria-describedby={desc('status', 'lf-status-h')}>
              <option value={302}>{tr('lnkStatus302')}</option>
              <option value={307}>{tr('lnkStatus307')}</option>
              <option value={301}>{tr('lnkStatus301')}</option>
              <option value={308}>{tr('lnkStatus308')}</option>
            </select>
            {@render error('status')}
            {#if draft.redirect_status === 301 || draft.redirect_status === 308}
              <p class="field-hint warn" id="lf-status-h"><Icon name="triangle-alert" />{tr('permanentHint')}</p>
            {/if}
          </div>
          <label class="check"><input class="switch" type="checkbox" bind:checked={draft.forward_query} /><span>{tr('lnkForwardQuery')}<small>{tr('lnkForwardQueryHint')}</small></span></label>
          <label class="check"><input class="switch" type="checkbox" bind:checked={draft.forward_path} /><span>{tr('lnkForwardPath')}<small>{tr('lnkForwardPathHint', { slug: draft.slug || 'slug' })}</small></span></label>
          <label class="check"><input class="switch" type="checkbox" bind:checked={draft.noindex} /><span>{tr('requestNoIndex')}<small>{tr('requestNoIndexHint')}</small></span></label>
          {#if mode === 'create'}
            <p class="field-hint">{tr('lnkRulesLater')}</p>
          {:else if linkId != null}
            <p class="field-hint">
              {tr('lnkRuleCount', { count: ctx.ruleCount })}
              <a href={href('links', linkId, { tab: 'rules' })}>{tr('lnkEditRules')}</a>
            </p>
          {/if}
        {/if}
      </div>
    </details>

    <details class="disclosure" bind:open={open.tracking}>
      <summary><Icon name="activity" />{ctx.monitoring ? tr('lnkSectionTracking') : tr('analytics')}<span class="meta">{trackingSummary}</span><Icon name="chevron-right" /></summary>
      <div class="disclosure-body">
        <label class="check"><input class="switch" type="checkbox" bind:checked={draft.analytics_enabled} /><span>{tr('analyticsEnabled')}<small>{tr('lnkAnalyticsHint')}</small></span></label>
        {#if ctx.monitoring}
          <label class="check"><input class="switch" type="checkbox" bind:checked={draft.monitor} /><span>{tr('lnkMonitor')}<small>{tr('lnkMonitorHint')}</small></span></label>
        {/if}
      </div>
    </details>
  </div>

  {#if errors.form}
    <div class="notice" data-tone="error" role="alert" id="lf-form-e" tabindex="-1"><Icon name="circle-alert" /><div><p>{tr(errors.form)}</p></div></div>
  {/if}
</fieldset>

<style>
  .form-fields {
    display: grid;
    gap: var(--space-5);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .sections {
    display: grid;
    gap: var(--space-2);
  }
  .row-2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  @media (max-width: 520px) {
    .row-2 {
      grid-template-columns: 1fr;
    }
  }
  .narrow {
    max-width: 200px;
  }
  .narrow-select {
    max-width: 320px;
  }
  .warn {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    color: var(--warning);
  }
  .warn :global(svg) {
    flex: none;
    width: 14px;
    height: 14px;
    margin-top: 2px;
  }
  .inline-hint {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 4px 6px 4px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
    font-size: var(--text-xs);
  }
  .inline-hint > :global(svg) {
    flex: none;
    width: 14px;
    height: 14px;
    color: var(--accent);
  }
  .inline-hint[data-tone='info'] > :global(svg) {
    color: var(--info);
  }
  .inline-hint > span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .inline-hint :global(.short-url) {
    font-size: var(--text-xs);
  }
  .inline-hint a {
    text-decoration: none;
  }
  .inline-hint code {
    color: var(--text);
  }
  .cleaner {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
  }
  .cleaner-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--text-sm);
  }
  .wrap {
    overflow-wrap: anywhere;
  }
</style>
