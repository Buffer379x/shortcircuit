<script lang="ts">
  // Settings > Operations: health checks, title lookup, scheduled backups, and notifications.
  // Switches apply at once. Intervals, retention, and notification events are staged and saved
  // through the unsaved-changes bar. The webhook and its Authorization header are write-only and
  // change only after a password confirmation. Each value shows where it comes from.
  import { onMount, tick } from 'svelte';
  import { ApiError } from '../../api';
  import type { NotificationEvent, OperationsSettings, SettingSource } from '../../api';
  import { problemMessage } from '../../format';
  import type { Messages } from '../../i18n';
  import Icon from '../../lib/Icon.svelte';
  import Dialog from '../../lib/ui/Dialog.svelte';
  import { app } from '../../lib/app.svelte';
  import { locale, tr } from '../../lib/locale.svelte';
  import { toast } from '../../lib/toast.svelte';

  const EVENTS: [NotificationEvent, keyof Messages][] = [
    ['link_health', 'opsEventLinkHealth'],
    ['link_expiring', 'opsEventLinkExpiring'],
    ['link_expired', 'opsEventLinkExpired'],
    ['click_milestone', 'opsEventClickMilestone'],
    ['backup_failed', 'opsEventBackupFailed'],
    ['job_failed', 'opsEventJobFailed'],
    ['storage_low', 'opsEventStorageLow'],
    ['security', 'opsEventSecurity'],
  ];
  const SOURCES: Record<SettingSource, keyof Messages> = {
    saved: 'opsSourceSaved',
    container: 'opsSourceContainer',
    default: 'opsSourceDefault',
  };

  type Field =
    | 'monitor_interval_hours' | 'monitor_priority_hours' | 'backup_interval_hours' | 'backup_keep'
    | 'notification_events' | 'notification_click_milestones' | 'notification_expiry_notice_hours';
  interface Draft {
    monitor_interval_hours: number | undefined;
    monitor_priority_hours: number | undefined;
    backup_interval_hours: number | undefined;
    backup_keep: number | undefined;
    notification_events: NotificationEvent[];
    notification_click_milestones: string;
    notification_expiry_notice_hours: number | undefined;
  }
  const FIELDS: Field[] = [
    'monitor_interval_hours', 'monitor_priority_hours', 'backup_interval_hours', 'backup_keep',
    'notification_events', 'notification_click_milestones', 'notification_expiry_notice_hours',
  ];
  // Which field receives focus for each validation error.
  const ERROR_FIELDS: Record<string, string> = {
    invalid_monitor_intervals: 'ops-interval',
    invalid_backup_interval: 'ops-backup-interval',
    invalid_backup_keep: 'ops-backup-keep',
    invalid_notification_events: 'ops-event-link_health',
    click_milestones_required: 'ops-milestones',
    invalid_click_milestones: 'ops-milestones',
    invalid_expiry_notice: 'ops-expiry',
  };

  let settings = $state.raw<OperationsSettings | null>(null);
  let draft = $state<Draft | null>(null);
  let loading = $state(true);
  let loadError = $state<keyof Messages | null>(null);
  let saving = $state(false);
  let switching = $state<string | null>(null);
  let fieldError = $state<{ id: string; message: keyof Messages } | null>(null);

  function fromSettings(value: OperationsSettings): Draft {
    return {
      monitor_interval_hours: value.monitor_interval_hours.value,
      monitor_priority_hours: value.monitor_priority_hours.value,
      backup_interval_hours: value.backup_interval_hours.value,
      backup_keep: value.backup_keep.value,
      notification_events: [...value.notification_events.value],
      notification_click_milestones: value.notification_click_milestones.value.join(', '),
      notification_expiry_notice_hours: value.notification_expiry_notice_hours.value,
    };
  }

  function same(field: Field, value: OperationsSettings, current: Draft): boolean {
    const stored = fromSettings(value)[field];
    const staged = current[field];
    if (field === 'notification_events') return [...(stored as string[])].sort().join() === [...(staged as string[])].sort().join();
    return JSON.stringify(stored) === JSON.stringify(staged);
  }

  const changed = $derived(settings && draft ? FIELDS.filter((field) => !same(field, settings!, draft!)) : []);
  const dirty = $derived(changed.length > 0);

  /** Applies a server response and keeps staged edits of other fields. */
  function apply(next: OperationsSettings) {
    const previous = settings;
    const staged = draft;
    const fresh = fromSettings(next);
    if (previous && staged) {
      for (const field of FIELDS) {
        if (!same(field, previous, staged)) (fresh as unknown as Record<Field, unknown>)[field] = staged[field];
      }
    }
    settings = next;
    draft = fresh;
  }

  onMount(() => { void load(); });

  async function load() {
    loading = true;
    loadError = null;
    try {
      const next = await app.api<OperationsSettings>('/settings/operations');
      settings = next;
      draft = fromSettings(next);
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  async function failField(code: string, message: keyof Messages) {
    const id = ERROR_FIELDS[code] ?? '';
    fieldError = { id, message };
    await tick();
    if (id) document.getElementById(id)?.focus();
  }

  function milestones(text: string): number[] | null {
    const values = text.split(',').map((part) => part.trim()).filter(Boolean).map(Number);
    return values.every((value) => Number.isSafeInteger(value) && value >= 1) ? values : null;
  }

  async function save() {
    if (!draft || saving) return;
    fieldError = null;
    const patch: Record<string, unknown> = {};
    for (const field of changed) {
      if (field === 'notification_click_milestones') {
        const values = milestones(draft.notification_click_milestones);
        if (!values) return failField('invalid_click_milestones', 'opsInvalidMilestones');
        patch[field] = values;
      } else if (field === 'notification_events') {
        if (!draft.notification_events.length) return failField('invalid_notification_events', 'opsInvalidEvents');
        patch[field] = draft.notification_events;
      } else {
        // An empty number field is sent as an invalid value, so the server names the problem.
        patch[field] = draft[field] ?? -1;
      }
    }
    saving = true;
    try {
      const next = await app.api<OperationsSettings>('/settings/operations', 'PATCH', patch);
      settings = next;
      draft = fromSettings(next);
      toast(tr('opsSaved'));
    } catch (error) {
      await failField(error instanceof ApiError ? error.code : '', problemMessage(error));
    } finally {
      saving = false;
    }
  }

  function discard() {
    if (settings) draft = fromSettings(settings);
    fieldError = null;
  }

  async function change(key: string, patch: Record<string, unknown>, done: string) {
    switching = key;
    try {
      apply(await app.api<OperationsSettings>('/settings/operations', 'PATCH', patch));
      toast(done);
    } catch (error) {
      toast(tr(problemMessage(error)), { tone: 'error' });
      // Show the stored state again, for example after a refused switch.
      if (settings) settings = { ...settings };
    } finally {
      switching = null;
    }
  }

  function toggle(field: 'monitor_enabled' | 'title_lookup_enabled', event: Event) {
    const enabled = (event.currentTarget as HTMLInputElement).checked;
    const done: keyof Messages = field === 'monitor_enabled' ? (enabled ? 'opsHealthOn' : 'opsHealthOff') : enabled ? 'opsTitlesOn' : 'opsTitlesOff';
    void change(field, { [field]: enabled }, tr(done));
  }

  function reset(field: string) {
    void change(field, { reset: [field] }, tr('opsResetDone'));
  }

  const hours = (value: number) => new Intl.NumberFormat(locale.language, { style: 'unit', unit: 'hour', unitDisplay: 'long' }).format(value);

  // ---------- Webhook ----------
  type TargetMode = 'change' | 'off' | 'reset';
  type TargetField = 'url' | 'auth' | 'password' | 'form';
  let targetOpen = $state(false);
  let targetMode = $state<TargetMode>('change');
  let targetUrl = $state('');
  let targetAuth = $state('');
  let removeAuth = $state(false);
  let targetPassword = $state('');
  let targetBusy = $state(false);
  let targetError = $state<{ field: TargetField; message: keyof Messages } | null>(null);
  const TARGET_FIELDS: Record<string, TargetField> = {
    invalid_notification_webhook: 'url',
    notification_webhook_not_public: 'url',
    invalid_notification_authorization: 'auth',
  };
  const TARGET_IDS: Record<TargetField, string> = { url: 'ops-target-url', auth: 'ops-target-auth', password: 'ops-target-password', form: 'ops-target-error' };

  function openTarget(mode: TargetMode) {
    targetMode = mode;
    targetUrl = '';
    targetAuth = '';
    removeAuth = false;
    targetPassword = '';
    targetError = null;
    targetOpen = true;
  }

  async function failTarget(field: TargetField, message: keyof Messages) {
    targetError = { field, message };
    await tick();
    document.getElementById(TARGET_IDS[field])?.focus();
  }

  async function saveTarget(event: SubmitEvent) {
    event.preventDefault();
    if (targetBusy) return;
    targetError = null;
    const patch: Record<string, unknown> = {};
    if (targetMode === 'off') patch.notification_webhook_url = '';
    else if (targetMode === 'reset') patch.reset = ['notification_webhook_url', 'notification_authorization'];
    else {
      if (targetUrl.trim()) patch.notification_webhook_url = targetUrl.trim();
      if (removeAuth) patch.notification_authorization = '';
      else if (targetAuth) patch.notification_authorization = targetAuth;
      if (!Object.keys(patch).length) return failTarget('url', 'opsTargetNothing');
    }
    if (!targetPassword) return failTarget('password', 'admPasswordRequired');
    targetBusy = true;
    let failure: [TargetField, keyof Messages] | null = null;
    try {
      await app.api<void>('/settings/operations/authorize', 'POST', { password: targetPassword });
      targetPassword = '';
      apply(await app.api<OperationsSettings>('/settings/operations', 'PATCH', patch));
      targetAuth = '';
      targetUrl = '';
      targetOpen = false;
      toast(tr(targetMode === 'off' ? 'opsTargetOffDone' : targetMode === 'reset' ? 'opsResetDone' : 'opsTargetSaved'));
    } catch (error) {
      const code = error instanceof ApiError ? error.code : '';
      if (code === 'invalid_current_password') failure = ['password', 'incorrectCurrentPassword'];
      else if (code === 'operation_authorization_required') failure = ['password', 'opsConfirmAgain'];
      else failure = [TARGET_FIELDS[code] ?? 'form', problemMessage(error)];
    } finally {
      targetBusy = false;
    }
    if (failure) await failTarget(...failure);
  }

  const targetTitle = $derived(targetMode === 'off' ? tr('opsTargetOffTitle') : targetMode === 'reset' ? tr('opsTargetResetTitle') : tr('opsTargetTitle'));
  const targetIntro = $derived(targetMode === 'off' ? tr('opsTargetOffIntro') : targetMode === 'reset' ? tr('opsTargetResetIntro') : tr('opsTargetIntro'));
  const resetLabel = $derived(settings?.notification_webhook.container_configured || settings?.notification_authorization.container_configured ? tr('opsResetContainer') : tr('opsResetDefault'));
  const targetAction = $derived(targetMode === 'off' ? tr('opsTargetOffAction') : targetMode === 'reset' ? resetLabel : tr('opsTargetSave'));
</script>

{#snippet source(field: string, value: { source: SettingSource; container?: unknown; container_configured?: boolean }, resettable = true)}
  {@const container = value.container_configured ?? (value.container !== null && value.container !== undefined)}
  <span class="setting-source">
    <span class="badge" data-tone={value.source === 'saved' ? 'info' : 'muted'}><Icon name={value.source === 'saved' ? 'database' : value.source === 'container' ? 'server' : 'settings'} />{tr(SOURCES[value.source])}</span>
    {#if value.source === 'saved' && resettable}
      <button class="btn" type="button" data-variant="ghost" data-size="sm" disabled={!!switching || saving} aria-busy={switching === field} onclick={() => reset(field)}>
        <Icon name="rotate-ccw" />{container ? tr('opsResetContainer') : tr('opsResetDefault')}
      </button>
    {/if}
  </span>
{/snippet}

{#snippet errorFor(id: string)}
  {#if fieldError && fieldError.id === id}<div class="panel-body"><p class="field-error" id="{id}-error" role="alert"><Icon name="circle-alert" />{locale.t[fieldError.message]}</p></div>{/if}
{/snippet}

{#if loadError}
  <section class="panel" aria-labelledby="st-operations">
    <header class="panel-head"><div class="panel-title"><h2 id="st-operations">{tr('stOperations')}</h2></div></header>
    <div class="panel-body">
      <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[loadError]}</p></div><button class="btn" data-size="sm" onclick={load}>{tr('tryAgain')}</button></div>
    </div>
  </section>
{:else if loading || !settings || !draft}
  <section class="panel" aria-labelledby="st-operations" aria-busy="true">
    <header class="panel-head"><div class="panel-title"><h2 id="st-operations">{tr('stOperations')}</h2></div></header>
    <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
  </section>
{:else}
  {@const s = settings}
  <section class="panel" aria-labelledby="ops-health" aria-busy={saving}>
    <header class="panel-head"><div class="panel-title"><h2 id="ops-health">{tr('opsHealth')}</h2><p>{tr('opsHealthHint')}</p></div></header>
    <div class="setting">
      <div>
        <b id="ops-monitor">{tr('opsHealthSwitch')}</b><p id="ops-monitor-hint">{tr('opsHealthSwitchHint')}</p>
        {@render source('monitor_enabled', s.monitor_enabled)}
      </div>
      <div><input id="ops-monitor-switch" class="switch" type="checkbox" checked={s.monitor_enabled.value} disabled={!!switching} onchange={(event) => toggle('monitor_enabled', event)} aria-labelledby="ops-monitor" aria-describedby="ops-monitor-hint" /></div>
    </div>
    <div class="setting">
      <div>
        <b><label for="ops-interval">{tr('opsInterval')}</label></b><p id="ops-interval-hint">{tr('opsIntervalHint')}</p>
        {@render source('monitor_interval_hours', s.monitor_interval_hours)}
      </div>
      <div><input id="ops-interval" class="input num hours" type="number" min="1" max="720" step="1" bind:value={draft.monitor_interval_hours} disabled={saving} aria-invalid={fieldError?.id === 'ops-interval' ? 'true' : undefined} aria-describedby={fieldError?.id === 'ops-interval' ? 'ops-interval-error ops-interval-hint' : 'ops-interval-hint'} /></div>
    </div>
    {@render errorFor('ops-interval')}
    <div class="setting">
      <div>
        <b><label for="ops-priority">{tr('opsPriority')}</label></b><p id="ops-priority-hint">{tr('opsPriorityHint')}</p>
        {@render source('monitor_priority_hours', s.monitor_priority_hours)}
      </div>
      <div><input id="ops-priority" class="input num hours" type="number" min="1" max="720" step="1" bind:value={draft.monitor_priority_hours} disabled={saving} aria-describedby="ops-priority-hint" /></div>
    </div>
  </section>

  <section class="panel" aria-labelledby="ops-titles">
    <header class="panel-head"><div class="panel-title"><h2 id="ops-titles">{tr('opsTitles')}</h2><p>{tr('opsTitlesHint')}</p></div></header>
    <div class="setting">
      <div>
        <b id="ops-title-lookup">{tr('opsTitlesSwitch')}</b><p id="ops-title-lookup-hint">{tr('opsTitlesSwitchHint')}</p>
        {@render source('title_lookup_enabled', s.title_lookup_enabled)}
      </div>
      <div><input id="ops-titles-switch" class="switch" type="checkbox" checked={s.title_lookup_enabled.value} disabled={!!switching} onchange={(event) => toggle('title_lookup_enabled', event)} aria-labelledby="ops-title-lookup" aria-describedby="ops-title-lookup-hint" /></div>
    </div>
  </section>

  <section class="panel" aria-labelledby="ops-backups" aria-busy={saving}>
    <header class="panel-head"><div class="panel-title"><h2 id="ops-backups">{tr('opsBackups')}</h2><p>{tr('opsBackupsHint')}</p></div></header>
    <div class="setting">
      <div>
        <b><label for="ops-backup-interval">{tr('opsBackupInterval')}</label></b><p id="ops-backup-interval-hint">{tr('opsBackupIntervalHint')}</p>
        {@render source('backup_interval_hours', s.backup_interval_hours)}
      </div>
      <div><input id="ops-backup-interval" class="input num hours" type="number" min="0" max="720" step="1" bind:value={draft.backup_interval_hours} disabled={saving} aria-invalid={fieldError?.id === 'ops-backup-interval' ? 'true' : undefined} aria-describedby="ops-backup-interval-hint" /></div>
    </div>
    {@render errorFor('ops-backup-interval')}
    <div class="setting">
      <div>
        <b><label for="ops-backup-keep">{tr('opsBackupKeep')}</label></b><p id="ops-backup-keep-hint">{tr('opsBackupKeepHint')}</p>
        {@render source('backup_keep', s.backup_keep)}
      </div>
      <div><input id="ops-backup-keep" class="input num hours" type="number" min="1" max="365" step="1" bind:value={draft.backup_keep} disabled={saving} aria-invalid={fieldError?.id === 'ops-backup-keep' ? 'true' : undefined} aria-describedby="ops-backup-keep-hint" /></div>
    </div>
    {@render errorFor('ops-backup-keep')}
  </section>

  <section class="panel" aria-labelledby="ops-notifications" aria-busy={saving}>
    <header class="panel-head"><div class="panel-title"><h2 id="ops-notifications">{tr('opsNotifications')}</h2><p>{tr('opsNotificationsHint')}</p></div></header>
    <div class="setting">
      <div>
        <b id="ops-webhook">{tr('opsWebhook')}</b><p>{tr('opsWebhookHint')}</p>
        {@render source('notification_webhook_url', s.notification_webhook, false)}
      </div>
      <div class="target">
        <span class="mono value" id="ops-webhook-value">{s.notification_webhook.configured ? (s.notification_webhook.origin ?? tr('opsConfigured')) : tr('opsWebhookNone')}</span>
        <span class="actions">
          <button class="btn" type="button" data-size="sm" onclick={() => openTarget('change')}><Icon name="webhook" />{tr('opsWebhookChange')}</button>
          {#if s.notification_webhook.configured}<button class="btn" type="button" data-variant="ghost" data-size="sm" onclick={() => openTarget('off')}>{tr('opsWebhookOff')}</button>{/if}
          {#if s.notification_webhook.source === 'saved' || s.notification_authorization.source === 'saved'}
            <button class="btn" type="button" data-variant="ghost" data-size="sm" onclick={() => openTarget('reset')}><Icon name="rotate-ccw" />{resetLabel}</button>
          {/if}
        </span>
      </div>
    </div>
    <div class="setting">
      <div>
        <b>{tr('opsAuthorization')}</b><p>{tr('opsAuthorizationHint')}</p>
        {@render source('notification_authorization', s.notification_authorization, false)}
      </div>
      <div><span class="value">{s.notification_authorization.configured ? tr('opsConfigured') : tr('opsNotConfigured')}</span></div>
    </div>
    {#if s.connector}
      <div class="setting">
        <div><b>{tr('opsConnector')}</b><p>{tr('opsConnectorHint')}</p></div>
        <div><span class="value">{tr('opsConfigured')}</span></div>
      </div>
    {/if}
    <div class="setting">
      <div>
        <b id="ops-events">{tr('opsEvents')}</b><p id="ops-events-hint">{tr('opsEventsHint')}</p>
        {@render source('notification_events', s.notification_events)}
      </div>
      <div>
        <fieldset class="events" aria-labelledby="ops-events" aria-describedby="ops-events-hint">
          {#each EVENTS as [event, label] (event)}
            <label class="check"><input id="ops-event-{event}" class="checkbox" type="checkbox" value={event} bind:group={draft.notification_events} disabled={saving} /><span>{tr(label)}</span></label>
          {/each}
        </fieldset>
      </div>
    </div>
    {@render errorFor('ops-event-link_health')}
    <div class="setting">
      <div>
        <b><label for="ops-milestones">{tr('opsMilestones')}</label></b><p id="ops-milestones-hint">{tr('opsMilestonesHint')}</p>
        {@render source('notification_click_milestones', s.notification_click_milestones)}
      </div>
      <div><input id="ops-milestones" class="input" inputmode="numeric" autocomplete="off" bind:value={draft.notification_click_milestones} disabled={saving} aria-invalid={fieldError?.id === 'ops-milestones' ? 'true' : undefined} aria-describedby="ops-milestones-hint" /></div>
    </div>
    {@render errorFor('ops-milestones')}
    <div class="setting">
      <div>
        <b><label for="ops-expiry">{tr('opsExpiry')}</label></b><p id="ops-expiry-hint">{tr('opsExpiryHint')} {hours(s.notification_expiry_notice_hours.value)}.</p>
        {@render source('notification_expiry_notice_hours', s.notification_expiry_notice_hours)}
      </div>
      <div><input id="ops-expiry" class="input num hours" type="number" min="1" max="720" step="1" bind:value={draft.notification_expiry_notice_hours} disabled={saving} aria-invalid={fieldError?.id === 'ops-expiry' ? 'true' : undefined} aria-describedby="ops-expiry-hint" /></div>
    </div>
    {@render errorFor('ops-expiry')}
    <footer class="panel-foot"><span class="field-hint">{tr('opsSourceHint')} {tr('opsEnvOnly')}</span></footer>
  </section>

  {#if fieldError && !fieldError.id}
    <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[fieldError.message]}</p></div></div>
  {/if}
  {#if dirty}
    <div class="savebar" role="region" aria-label={tr('admUnsaved')}>
      <Icon name="circle-dot" /><span>{tr('admUnsaved')}</span>
      <span class="spacer"></span>
      <button class="btn" data-variant="ghost" disabled={saving} onclick={discard}>{tr('admDiscard')}</button>
      <button class="btn" data-variant="primary" disabled={saving} aria-busy={saving} onclick={save}>
        {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{tr('save')}
      </button>
    </div>
  {/if}
{/if}

<Dialog bind:open={targetOpen} title={targetTitle} description={targetIntro} width={500} busy={targetBusy}>
  <form id="ops-target" class="form" onsubmit={saveTarget} novalidate aria-busy={targetBusy}>
    <input class="visually-hidden" type="text" name="username" autocomplete="username" value={app.user?.username ?? ''} readonly tabindex="-1" aria-hidden="true" />
    {#if targetMode === 'change'}
      <div class="field">
        <label class="field-label" for="ops-target-url">{tr('opsTargetUrl')}</label>
        <input id="ops-target-url" class="input mono" type="url" inputmode="url" autocomplete="off" spellcheck="false" placeholder={settings?.notification_webhook.origin ?? 'https://'} bind:value={targetUrl} disabled={targetBusy} aria-invalid={targetError?.field === 'url' ? 'true' : undefined} aria-describedby={targetError?.field === 'url' ? 'ops-target-url-error ops-target-url-hint' : 'ops-target-url-hint'} />
        {#if targetError?.field === 'url'}<p class="field-error" id="ops-target-url-error"><Icon name="circle-alert" />{locale.t[targetError.message]}</p>{/if}
        <p class="field-hint" id="ops-target-url-hint">{tr('opsTargetUrlHint')}</p>
      </div>
      <div class="field">
        <label class="field-label" for="ops-target-auth">{tr('opsTargetAuth')}</label>
        <input id="ops-target-auth" class="input mono" type="password" autocomplete="off" spellcheck="false" bind:value={targetAuth} disabled={targetBusy || removeAuth} aria-invalid={targetError?.field === 'auth' ? 'true' : undefined} aria-describedby={targetError?.field === 'auth' ? 'ops-target-auth-error ops-target-auth-hint' : 'ops-target-auth-hint'} />
        {#if targetError?.field === 'auth'}<p class="field-error" id="ops-target-auth-error"><Icon name="circle-alert" />{locale.t[targetError.message]}</p>{/if}
        <p class="field-hint" id="ops-target-auth-hint">{tr('opsTargetAuthHint')}</p>
        {#if settings?.notification_authorization.configured}
          <label class="check"><input class="checkbox" type="checkbox" bind:checked={removeAuth} disabled={targetBusy} onchange={() => { if (removeAuth) targetAuth = ''; }} /><span>{tr('opsTargetAuthRemove')}</span></label>
        {/if}
      </div>
    {/if}
    <div class="field">
      <label class="field-label" for="ops-target-password">{tr('currentPassword')}</label>
      <input id="ops-target-password" class="input" type="password" name="current_password" autocomplete="current-password" bind:value={targetPassword} disabled={targetBusy} aria-invalid={targetError?.field === 'password' ? 'true' : undefined} aria-describedby={targetError?.field === 'password' ? 'ops-target-password-error' : undefined} />
      {#if targetError?.field === 'password'}<p class="field-error" id="ops-target-password-error"><Icon name="circle-alert" />{locale.t[targetError.message]}</p>{/if}
    </div>
    {#if targetError?.field === 'form'}<div class="notice" data-tone="error" role="alert" id="ops-target-error" tabindex="-1"><Icon name="circle-alert" /><div><p>{locale.t[targetError.message]}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={targetBusy} onclick={() => (targetOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="ops-target" data-variant={targetMode === 'off' ? 'danger' : 'primary'} disabled={targetBusy} aria-busy={targetBusy}>
      {#if targetBusy}<span class="spinner" aria-hidden="true"></span>{/if}{targetAction}
    </button>
  {/snippet}
</Dialog>

<style>
  .hours {
    max-width: 120px;
  }
  .value {
    font-size: var(--text-md);
    overflow-wrap: anywhere;
  }
  .target {
    flex-direction: column;
    align-items: flex-end;
    gap: var(--space-2);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .events {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  @container (max-width: 640px) {
    .target {
      align-items: flex-start;
    }
    .actions {
      justify-content: flex-start;
    }
  }
</style>
