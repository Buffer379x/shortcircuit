<script lang="ts">
  // Account (password), appearance (theme, language, time zone), privacy and analytics settings with
  // the clear-data danger zone, and what the API offers about sign-in and sessions. Language, time
  // zone, and analytics changes are staged and saved through the unsaved-changes bar.
  import { onMount, tick } from 'svelte';
  import { ApiError } from '../api';
  import type { User } from '../api';
  import { passwordAllowed, problemMessage } from '../format';
  import type { Language, Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import type { IconName } from '../lib/icons';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import PublicPages from './settings/PublicPages.svelte';
  import Operations from './settings/Operations.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import { app } from '../lib/app.svelte';
  import { locale, setLanguage, tr } from '../lib/locale.svelte';
  import { route, setParams } from '../lib/router.svelte';
  import { theme, setTheme } from '../lib/theme.svelte';
  import { toast } from '../lib/toast.svelte';
  import { defaultTimeZone } from '../timezone';

  let { timeZone = $bindable(), onPasswordChanged }: { timeZone?: string; onPasswordChanged: (user?: User) => Promise<void> } = $props();

  type Section = 'account' | 'appearance' | 'privacy' | 'pages' | 'operations' | 'security';
  const SECTIONS: [Section, keyof Messages, IconName][] = [
    ['account', 'account', 'user'], ['appearance', 'admAppearance', 'palette'],
    ['privacy', 'admPrivacy', 'eye-off'], ['pages', 'stPublicPages', 'file-text'],
    ['operations', 'stOperations', 'server'], ['security', 'admSecurity', 'shield'],
  ];
  const initial = route.params.get('section') as Section;
  let section = $state<Section>(SECTIONS.some(([key]) => key === initial) ? initial : 'account');
  $effect(() => setParams({ section: section === 'account' ? null : section }));

  // ---------- Appearance ----------
  const browserZone = defaultTimeZone();
  const zones = [...new Set(['UTC', browserZone, ...(Intl.supportedValuesOf?.('timeZone') ?? [])])];
  let languageDraft = $state<Language>(locale.language);
  let zoneDraft = $state(timeZone ?? '');
  const zoneOptions = $derived(zoneDraft && !zones.includes(zoneDraft) ? [...zones, zoneDraft] : zones);

  // ---------- Privacy and analytics ----------
  interface AnalyticsSettings {
    enabled: boolean;
    referrers: boolean;
    classify: boolean;
    honor_dnt: boolean;
    detail_days: number;
    aggregate_days: number;
  }
  type AnalyticsDraft = Omit<AnalyticsSettings, 'detail_days' | 'aggregate_days'> & { detail_days: number | undefined; aggregate_days: number | undefined };
  const switches: [keyof AnalyticsSettings & ('enabled' | 'referrers' | 'classify' | 'honor_dnt'), keyof Messages, keyof Messages][] = [
    ['enabled', 'analyticsEnabled', 'admRecordHint'],
    ['honor_dnt', 'analyticsHonorDnt', 'admDntHint'],
    ['referrers', 'analyticsReferrers', 'admReferrersHint'],
    ['classify', 'analyticsClassify', 'analyticsClassificationHint'],
  ];
  let analytics = $state.raw<AnalyticsSettings | null>(null);
  let analyticsDraft = $state<AnalyticsDraft | null>(null);
  let analyticsLoading = $state(true);
  let analyticsError = $state<keyof Messages | null>(null);
  let retentionError = $state<keyof Messages | null>(null);
  let saveError = $state<keyof Messages | null>(null);
  let saving = $state(false);

  const analyticsDirty = $derived(!!analytics && !!analyticsDraft && JSON.stringify(analytics) !== JSON.stringify({ ...analyticsDraft }));
  const dirty = $derived(languageDraft !== locale.language || zoneDraft !== (timeZone ?? '') || analyticsDirty);

  onMount(() => { void loadAnalytics(); });

  async function loadAnalytics() {
    analyticsLoading = true;
    analyticsError = null;
    try {
      analytics = await app.api<AnalyticsSettings>('/analytics/settings');
      analyticsDraft = { ...analytics };
    } catch (error) {
      analyticsError = problemMessage(error);
    } finally {
      analyticsLoading = false;
    }
  }

  function discard() {
    languageDraft = locale.language;
    zoneDraft = timeZone ?? '';
    if (analytics) analyticsDraft = { ...analytics };
    retentionError = null;
    saveError = null;
  }

  async function save() {
    if (saving || clearing) return;
    let analyticsSaved = false;
    retentionError = null;
    saveError = null;
    if (analyticsDirty && analyticsDraft) {
      const detail = analyticsDraft.detail_days ?? 0;
      const aggregate = analyticsDraft.aggregate_days ?? 0;
      const valid = [detail, aggregate].every((days) => Number.isInteger(days) && days >= 1 && days <= 3650);
      if (!valid || aggregate < detail) {
        retentionError = 'invalidAnalyticsRetention';
        section = 'privacy';
        await tick();
        document.getElementById('st-detail')?.focus();
        return;
      }
      saving = true;
      try {
        analytics = await app.api<AnalyticsSettings>('/analytics/settings', 'PATCH', { ...analyticsDraft, detail_days: detail, aggregate_days: aggregate });
        analyticsDraft = { ...analytics };
        analyticsSaved = true;
      } catch (error) {
        saving = false;
        if (error instanceof ApiError && error.code === 'invalid_retention') {
          retentionError = 'invalidAnalyticsRetention';
          section = 'privacy';
          await tick();
          document.getElementById('st-detail')?.focus();
        } else {
          saveError = problemMessage(error);
        }
        return;
      }
      saving = false;
    }
    if (languageDraft !== locale.language) setLanguage(languageDraft);
    if (zoneDraft !== (timeZone ?? '')) {
      timeZone = zoneDraft || undefined;
      try {
        if (zoneDraft) localStorage.setItem('shortcircuit-timezone', zoneDraft);
        else localStorage.removeItem('shortcircuit-timezone');
      } catch {
        /* The choice still applies to this page. */
      }
    }
    toast(analyticsSaved ? tr('analyticsSettingsSaved') : tr('admSettingsSaved'));
  }

  // ---------- Password ----------
  let passwordOpen = $state(false);
  let current = $state('');
  let nextPassword = $state('');
  let changing = $state(false);
  let passwordError = $state<{ field: 'current' | 'next'; message: keyof Messages } | null>(null);
  const nextOk = $derived(passwordAllowed(nextPassword));
  const nextLength = $derived([...nextPassword].length);

  function openPassword() {
    current = '';
    nextPassword = '';
    passwordError = null;
    passwordOpen = true;
  }

  async function failPassword(field: 'current' | 'next', message: keyof Messages) {
    passwordError = { field, message };
    await tick();
    document.getElementById(field === 'current' ? 'st-current' : 'st-new')?.focus();
  }

  async function changePassword(event: SubmitEvent) {
    event.preventDefault();
    if (changing) return;
    passwordError = null;
    if (!current) return failPassword('current', 'admPasswordRequired');
    if (!passwordAllowed(nextPassword)) return failPassword('next', 'invalidPassword');
    changing = true;
    let message: keyof Messages | null = null;
    try {
      const updated = await app.api<User | undefined>('/auth/password', 'POST', { current_password: current, password: nextPassword });
      current = '';
      nextPassword = '';
      await onPasswordChanged(updated);
      passwordOpen = false;
      toast(tr('passwordChanged'), { body: tr('admOtherSessions') });
    } catch (failure) {
      message = problemMessage(failure, 'invalidCredentials');
    } finally {
      changing = false;
    }
    if (message === 'invalidPassword') await failPassword('next', message);
    else if (message === 'invalidCredentials') await failPassword('current', 'incorrectCurrentPassword');
    else if (message) await failPassword('current', message);
  }

  // ---------- Clear analytics ----------
  let clearOpen = $state(false);
  let confirmText = $state('');
  let clearing = $state(false);
  let clearError = $state<keyof Messages | null>(null);
  const confirmWord = $derived(tr('admClearWord'));
  const confirmMatches = $derived(confirmText.trim().toLowerCase() === confirmWord.toLowerCase());

  function openClear() {
    confirmText = '';
    clearError = null;
    clearOpen = true;
  }

  async function clearData(event: SubmitEvent) {
    event.preventDefault();
    if (!confirmMatches || clearing || saving) return;
    clearing = true;
    clearError = null;
    try {
      await app.api<void>('/analytics/data', 'DELETE');
      clearOpen = false;
      toast(tr('analyticsDataCleared'));
    } catch (error) {
      clearError = problemMessage(error);
    } finally {
      clearing = false;
    }
  }
</script>

<div class="page">
  <PageHeader title={tr('navSettings')} description={tr('admSettingsDescription')} />

  <div class="layout">
    <nav class="subnav" aria-label={tr('admSettingsSections')}>
      {#each SECTIONS as [key, label, icon] (key)}
        <button class="nav-item" aria-current={section === key ? 'page' : undefined} onclick={() => (section = key)}><Icon name={icon} /><span>{tr(label)}</span></button>
      {/each}
    </nav>

    <div class="stack sections">
      {#if section === 'account'}
        <section class="panel" aria-labelledby="st-account">
          <header class="panel-head"><div class="panel-title"><h2 id="st-account">{tr('account')}</h2></div></header>
          <div class="setting">
            <div><b>{tr('username')}</b><p>{tr('admUsernameHint')}</p></div>
            <div><span class="value">{app.user?.username}</span></div>
          </div>
          <div class="setting">
            <div><b>{tr('password')}</b><p>{tr('admPasswordHint')}</p></div>
            <div><button class="btn" onclick={openPassword}><Icon name="key-round" />{tr('changePassword')}</button></div>
          </div>
        </section>
      {:else if section === 'appearance'}
        <section class="panel" aria-labelledby="st-appearance">
          <header class="panel-head"><div class="panel-title"><h2 id="st-appearance">{tr('admAppearance')}</h2><p>{tr('admStoredInBrowser')}</p></div></header>
          <div class="setting">
            <div><b id="st-theme">{tr('theme')}</b><p>{tr('admThemeHint')}</p></div>
            <div>
              <div class="seg" role="radiogroup" aria-labelledby="st-theme">
                {#each [['dark', 'moon', 'themeDark'], ['light', 'sun', 'themeLight'], ['system', 'monitor', 'themeSystem']] as const as [value, icon, label] (value)}
                  <button type="button" role="radio" aria-checked={theme.choice === value} onclick={() => setTheme(value)}><Icon name={icon} />{tr(label)}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="setting">
            <div><b><label for="st-language">{tr('language')}</label></b></div>
            <div>
              <select id="st-language" class="select narrow" bind:value={languageDraft}>
                <option value="en" lang="en">English</option>
                <option value="de" lang="de">Deutsch</option>
              </select>
            </div>
          </div>
          <div class="setting">
            <div><b><label for="st-zone">{tr('admTimeZone')}</label></b><p id="st-zone-hint">{tr('displayTimezoneHint')}</p></div>
            <div>
              <select id="st-zone" class="select wide" bind:value={zoneDraft} aria-describedby="st-zone-hint">
                <option value="">{tr('browserTimezone')} · {browserZone}</option>
                {#each zoneOptions as name (name)}<option value={name}>{name.replaceAll('_', ' ')}</option>{/each}
              </select>
            </div>
          </div>
        </section>
      {:else if section === 'privacy'}
        <section class="panel" aria-labelledby="st-privacy" aria-busy={analyticsLoading || saving}>
          <header class="panel-head"><div class="panel-title"><h2 id="st-privacy">{tr('admPrivacy')}</h2><p>{tr('admPrivacyHint')}</p></div></header>
          {#if analyticsError}
            <div class="panel-body">
              <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[analyticsError]}</p></div><button class="btn" data-size="sm" onclick={loadAnalytics}>{tr('tryAgain')}</button></div>
            </div>
          {:else if analyticsLoading || !analyticsDraft}
            <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
          {:else}
            {#each switches as [key, label, hint] (key)}
              <div class="setting">
                <div><b id="st-{key}">{tr(label)}</b><p id="st-{key}-hint">{tr(hint)}</p></div>
                <div><input class="switch" type="checkbox" bind:checked={analyticsDraft[key]} disabled={saving || clearing} aria-labelledby="st-{key}" aria-describedby="st-{key}-hint" /></div>
              </div>
            {/each}
            <div class="setting">
              <div><b><label for="st-detail">{tr('analyticsDetailRetention')}</label></b><p id="st-detail-hint">{tr('admDetailHint')}</p></div>
              <div><input id="st-detail" class="input num days" type="number" min="1" max="3650" step="1" bind:value={analyticsDraft.detail_days} disabled={saving || clearing} aria-invalid={retentionError ? 'true' : undefined} aria-describedby={retentionError ? 'st-retention-error st-detail-hint' : 'st-detail-hint'} /></div>
            </div>
            <div class="setting">
              <div><b><label for="st-aggregate">{tr('analyticsAggregateRetention')}</label></b><p id="st-aggregate-hint">{tr('admAggregateHint')}</p></div>
              <div><input id="st-aggregate" class="input num days" type="number" min="1" max="3650" step="1" bind:value={analyticsDraft.aggregate_days} disabled={saving || clearing} aria-invalid={retentionError ? 'true' : undefined} aria-describedby={retentionError ? 'st-retention-error st-aggregate-hint' : 'st-aggregate-hint'} /></div>
            </div>
            {#if retentionError}<div class="panel-body tight"><p class="field-error" id="st-retention-error" role="alert"><Icon name="circle-alert" />{locale.t[retentionError]}</p></div>{/if}
          {/if}
        </section>
        <section class="panel danger" aria-labelledby="st-danger">
          <header class="panel-head"><div class="panel-title"><h2 id="st-danger">{tr('admDeleteAnalytics')}</h2></div></header>
          <div class="setting">
            <div><b>{tr('analyticsClearData')}</b><p>{tr('admClearHint')}</p></div>
            <div><button class="btn" data-variant="danger-quiet" disabled={saving || clearing} onclick={openClear}><Icon name="trash-2" />{tr('admClearOpen')}</button></div>
          </div>
        </section>
      {:else if section === 'pages'}
        <PublicPages />
      {:else if section === 'operations'}
        <Operations />
      {:else}
        <section class="panel" aria-labelledby="st-sessions">
          <header class="panel-head"><div class="panel-title"><h2 id="st-sessions">{tr('admSessions')}</h2><p>{tr('admSessionsHint', { name: app.user?.username ?? '' })}</p></div></header>
          <dl class="facts panel-body">
            <dt>{tr('admSessionTimeout')}</dt><dd>{tr('admSessionTimeoutValue')}</dd>
            <dt>{tr('admPasswordChangeEffect')}</dt><dd>{tr('admPasswordChangeEffectValue')}</dd>
            <dt>{tr('admSignInLimit')}</dt><dd>{tr('admSignInLimitValue')}</dd>
          </dl>
        </section>
        <section class="panel" aria-labelledby="st-recovery">
          <header class="panel-head"><div class="panel-title"><h2 id="st-recovery">{tr('admRecovery')}</h2></div></header>
          <div class="panel-body"><p class="secondary small">{tr('admRecoveryText1')} <code>shortcircuit recovery-token</code> {tr('admRecoveryText2')}</p></div>
        </section>
      {/if}

      {#if saveError && dirty}
        <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[saveError]}</p></div></div>
      {/if}
      {#if dirty}
        <div class="savebar" role="region" aria-label={tr('admUnsaved')}>
          <Icon name="circle-dot" /><span>{tr('admUnsaved')}</span>
          <span class="spacer"></span>
          <button class="btn" data-variant="ghost" disabled={saving} onclick={discard}>{tr('admDiscard')}</button>
          <button class="btn" data-variant="primary" disabled={saving || clearing} aria-busy={saving} onclick={save}>
            {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{tr('save')}
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<Dialog bind:open={passwordOpen} title={tr('changePassword')} description={tr('admPasswordHint')} width={460} busy={changing}>
  <form id="st-password" class="form" onsubmit={changePassword} novalidate aria-busy={changing}>
    <input class="visually-hidden" type="text" name="username" autocomplete="username" value={app.user?.username ?? ''} readonly tabindex="-1" aria-hidden="true" />
    <div class="field">
      <label class="field-label" for="st-current">{tr('currentPassword')}</label>
      <input id="st-current" class="input" type="password" autocomplete="current-password" bind:value={current} disabled={changing} aria-invalid={passwordError?.field === 'current' ? 'true' : undefined} aria-describedby={passwordError?.field === 'current' ? 'st-current-error' : undefined} />
      {#if passwordError?.field === 'current'}<p class="field-error" id="st-current-error"><Icon name="circle-alert" />{locale.t[passwordError.message]}</p>{/if}
    </div>
    <div class="field">
      <label class="field-label" for="st-new">{tr('newPassword')}</label>
      <input id="st-new" class="input" type="password" autocomplete="new-password" bind:value={nextPassword} disabled={changing} aria-invalid={passwordError?.field === 'next' ? 'true' : undefined} aria-describedby={passwordError?.field === 'next' ? 'st-new-error st-new-rule' : 'st-new-rule'} />
      {#if passwordError?.field === 'next'}<p class="field-error" id="st-new-error"><Icon name="circle-alert" />{locale.t[passwordError.message]}</p>{/if}
      <p class="field-hint rule" id="st-new-rule" data-ok={nextOk || undefined}>{#if nextOk}<Icon name="check" />{/if}{nextLength ? tr('admPasswordRuleCount', { count: nextLength }) : tr('admPasswordRule')}</p>
    </div>
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={changing} onclick={() => (passwordOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="st-password" data-variant="primary" disabled={changing} aria-busy={changing}>
      {#if changing}<span class="spinner" aria-hidden="true"></span>{/if}{tr('changePassword')}
    </button>
  {/snippet}
</Dialog>

<Dialog bind:open={clearOpen} title={tr('admClearTitle')} width={460} busy={clearing}>
  <form id="st-clear" class="form" onsubmit={clearData} novalidate aria-busy={clearing}>
    <p class="secondary small" id="st-clear-hint">{tr('analyticsClearHint')} {tr('admClearTypePrefix')} <b>{confirmWord}</b> {tr('admClearTypeSuffix')}</p>
    <input class="input" bind:value={confirmText} disabled={clearing} autocomplete="off" spellcheck="false" aria-label={tr('admConfirmation')} aria-describedby="st-clear-hint" />
    {#if clearError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[clearError]}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={clearing} onclick={() => (clearOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="st-clear" data-variant="danger" disabled={!confirmMatches || clearing} aria-busy={clearing}>
      {#if clearing}<span class="spinner" aria-hidden="true"></span>{/if}{clearing ? tr('analyticsClearing') : tr('analyticsClearData')}
    </button>
  {/snippet}
</Dialog>

<style>
  .layout {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: var(--space-8);
    align-items: start;
  }
  .subnav {
    position: sticky;
    top: calc(var(--topbar-height) + var(--space-6));
    display: grid;
    gap: 2px;
  }
  .subnav .nav-item {
    border: 0;
    background: transparent;
    text-align: left;
    font-size: var(--text-sm);
  }
  .subnav .nav-item[aria-current='page'] {
    background: var(--surface);
  }
  .subnav .nav-item[aria-current='page']::before {
    display: none;
  }
  .sections {
    container-type: inline-size;
  }
  .value {
    font-size: var(--text-md);
  }
  .narrow {
    max-width: 220px;
  }
  .wide {
    max-width: 300px;
  }
  .days {
    max-width: 120px;
  }
  .small {
    font-size: var(--text-sm);
  }
  .tight {
    padding-top: 0;
  }
  .danger {
    border-color: color-mix(in srgb, var(--error) 30%, var(--border-subtle));
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  .rule {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .rule[data-ok] {
    color: var(--success);
  }
  .rule :global(svg) {
    width: 13px;
    height: 13px;
  }
  .savebar {
    position: sticky;
    bottom: var(--space-4);
    z-index: var(--z-sticky);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 10px 10px 10px 16px;
    border-radius: var(--radius-lg);
    background: var(--surface-raised);
    box-shadow: var(--shadow-popover);
    font-size: var(--text-sm);
    animation: pop-in var(--duration) var(--ease-out);
  }
  .savebar > :global(svg) {
    width: 14px;
    height: 14px;
    color: var(--warning);
  }
  .savebar .spacer {
    flex: 1;
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
    }
    .subnav {
      position: static;
      display: flex;
      overflow-x: auto;
      border-bottom: 1px solid var(--border);
      padding-bottom: var(--space-2);
    }
    .subnav .nav-item {
      flex: none;
    }
  }
</style>
