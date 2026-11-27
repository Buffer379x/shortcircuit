<script lang="ts">
  // Instance status, backups and staged restore, notifications, background jobs, and the audit log.
  // Every figure comes from the API; the tab and the audit filter live in the URL.
  import { onMount, tick } from 'svelte';
  import { ApiError } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import type { IconName } from '../lib/icons';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import { app } from '../lib/app.svelte';
  import { num } from '../lib/fmt';
  import { locale, tr } from '../lib/locale.svelte';
  import { route, setParams } from '../lib/router.svelte';
  import type { StateView } from '../lib/status';
  import { toast } from '../lib/toast.svelte';
  import { tip } from '../lib/tooltip';
  import { useTimeZone } from '../timezone';

  type Tab = 'status' | 'backups' | 'notifications' | 'jobs' | 'audit';
  const TABS: [Tab, keyof Messages, IconName][] = [
    ['status', 'admTabStatus', 'gauge'], ['backups', 'admTabBackups', 'database'], ['notifications', 'notifications', 'bell'],
    ['jobs', 'admTabJobs', 'refresh-cw'], ['audit', 'auditLog', 'file-text'],
  ];
  const tabIds = TABS.map(([key]) => key);
  const initial = route.params.get('tab') as Tab;
  let tab = $state<Tab>(tabIds.includes(initial) ? initial : 'status');
  $effect(() => setParams({ tab: tab === 'status' ? null : tab }));

  function tabKeys(event: KeyboardEvent) {
    const index = tabIds.indexOf(tab);
    const next = event.key === 'ArrowRight' ? index + 1 : event.key === 'ArrowLeft' ? index - 1 : event.key === 'Home' ? 0 : event.key === 'End' ? tabIds.length - 1 : null;
    if (next === null) return;
    event.preventDefault();
    tab = tabIds[(next + tabIds.length) % tabIds.length];
    void tick().then(() => document.getElementById(`sys-tab-${tab}`)?.focus());
  }

  const readTimeZone = useTimeZone();
  const request = new AbortController();

  // ---------- Diagnostics, backups, restore ----------
  const restoreLabels = {
    none: 'systemRestoreNone', pending_restart: 'systemRestorePending', applied: 'systemRestoreApplied',
    failed: 'systemRestoreFailed', interrupted: 'systemRestoreInterrupted', unavailable: 'systemRestoreUnavailable',
  } as const;
  const problemLabels: Record<string, keyof Messages> = {
    invalid_current_password: 'systemIncorrectPassword',
    operation_authorization_required: 'systemAuthorizationRequired',
    restore_already_pending: 'systemRestoreAlreadyPending',
    invalid_snapshot: 'systemInvalidSnapshot',
    no_pending_restore: 'systemNoPendingRestore',
    snapshot_too_large: 'systemSnapshotTooLarge',
    system_operation_in_progress: 'systemSystemBusy',
    invalid_snapshot_upload: 'systemSnapshotUploadFailed',
    snapshot_upload_timed_out: 'systemSnapshotUploadFailed',
    unsupported_snapshot_media_type: 'systemSnapshotUploadFailed',
  };

  interface Diagnostics {
    version: string;
    build_revision: string | null;
    started_at: number | null;
    uptime_seconds: number;
    process_rss_bytes: number | null;
    sqlite_version: string;
    schema_version: number;
    database_bytes: number;
    link_count: number;
    raw_event_count: number;
    analytics_enabled: boolean;
    scheduled_backups: boolean;
    last_backup_status: 'verified' | 'failed' | null;
    last_verified_backup_at: number | null;
    monitoring_enabled: boolean;
    last_monitor_check_at: number | null;
    system_operation_in_progress: boolean;
    restore_status: keyof typeof restoreLabels;
    max_snapshot_bytes: number;
    storage: { status: 'healthy' | 'low' | 'unavailable'; available_bytes: number | null; total_bytes: number | null; low_threshold_bytes: number };
    cpu: {
      window_ms: number; available_cores: number | null;
      process: 'measured' | 'unavailable'; process_percent: number | null;
      container: 'measured' | 'unavailable'; container_percent: number | null;
    };
    container_memory: { status: 'measured' | 'unavailable'; current_bytes: number | null; limit_bytes: number | null };
  }

  type Action = 'backup' | 'upload' | 'discard';
  const actionLabels: Record<Action, keyof Messages> = { backup: 'systemDownload', upload: 'systemStage', discard: 'systemDiscard' };
  const actionTitles: Record<Action, keyof Messages> = { backup: 'systemDownload', upload: 'admRestoreTitle', discard: 'systemDiscard' };

  let diagnostics = $state.raw<Diagnostics | null>(null);
  let diagnosticsError = $state.raw<unknown>(null);
  let refreshing = $state(false);
  let action = $state<Action | null>(null);
  let actionOpen = $state(false);
  let busy = $state<'auth' | 'transfer' | null>(null);
  let password = $state('');
  let confirmed = $state(false);
  let selected = $state.raw<File | null>(null);
  let fileInput = $state<HTMLInputElement>();
  let operationError = $state.raw<unknown>(null);
  let passwordMissing = $state(false);
  const pending = $derived(diagnostics?.restore_status === 'pending_restart');
  const tooLarge = $derived(!!selected && !!diagnostics && selected.size > diagnostics.max_snapshot_bytes);
  const passwordInvalid = $derived(passwordMissing || (operationError instanceof ApiError && operationError.code === 'invalid_current_password'));

  onMount(() => {
    void loadDiagnostics();
    return () => request.abort();
  });

  async function loadDiagnostics() {
    if (refreshing) return;
    refreshing = true;
    diagnosticsError = null;
    try {
      diagnostics = await app.api<Diagnostics>('/system', 'GET', undefined, 'json', request.signal);
    } catch (error) {
      if (!request.signal.aborted) diagnosticsError = error;
    } finally {
      refreshing = false;
    }
  }

  function bytes(value: number) {
    const unit = value >= 1024 ** 3 ? 'GiB' : 'MiB';
    const scale = unit === 'GiB' ? 1024 ** 3 : 1024 ** 2;
    return `${new Intl.NumberFormat(locale.language, { maximumFractionDigits: 2 }).format(value / scale)} ${unit}`;
  }

  function cpu(percent: number | null, windowMs: number) {
    if (percent === null) return tr('systemUnknown');
    return tr('systemCpuValue', {
      percent: new Intl.NumberFormat(locale.language, { style: 'percent', maximumFractionDigits: 1 }).format(percent / 100),
      window: new Intl.NumberFormat(locale.language, { style: 'unit', unit: 'millisecond' }).format(windowMs),
    });
  }

  function storageValue(storage: Diagnostics['storage']) {
    if (storage.available_bytes === null || storage.total_bytes === null) return tr('systemUnknown');
    return tr('systemStorageValue', { available: bytes(storage.available_bytes), total: bytes(storage.total_bytes) });
  }

  function containerMemory(memory: Diagnostics['container_memory']) {
    if (memory.current_bytes === null) return tr('systemUnknown');
    if (memory.limit_bytes === null) return tr('systemContainerMemoryUnlimited', { current: bytes(memory.current_bytes) });
    return tr('systemContainerMemoryValue', { current: bytes(memory.current_bytes), limit: bytes(memory.limit_bytes) });
  }

  function dateTime(value: number | null, seconds = true) {
    if (value === null) return tr('systemUnknown');
    const time = new Date(value * 1000);
    if (!Number.isFinite(time.getTime())) return tr('systemUnknown');
    return new Intl.DateTimeFormat(locale.language, {
      year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
      ...(seconds ? { second: '2-digit' } : {}), timeZone: readTimeZone(), timeZoneName: 'short',
    }).format(time);
  }

  function uptime(value: number) {
    const [amount, unit] = value >= 86_400 ? [value / 86_400, 'day'] : value >= 3600 ? [value / 3600, 'hour']
      : value >= 60 ? [value / 60, 'minute'] : [value, 'second'];
    return new Intl.NumberFormat(locale.language, { style: 'unit', unit, unitDisplay: 'long', maximumFractionDigits: 1 }).format(amount);
  }

  function errorMessage(error: unknown) {
    if (error instanceof ApiError && Object.hasOwn(problemLabels, error.code)) return tr(problemLabels[error.code]);
    return tr(problemMessage(error));
  }

  function choose(next: Action) {
    password = '';
    confirmed = false;
    operationError = null;
    passwordMissing = false;
    if (next === 'upload') {
      selected = null;
      if (fileInput) fileInput.value = '';
    }
    action = next;
    actionOpen = true;
  }

  function closeAction() {
    if (busy) return;
    actionOpen = false;
  }

  function resetAction() {
    password = '';
    confirmed = false;
    operationError = null;
    passwordMissing = false;
  }

  function downloadBackup() {
    const anchor = document.createElement('a');
    anchor.href = '/api/v1/system/backup';
    anchor.download = '';
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
  }

  async function submitAction(event: SubmitEvent) {
    event.preventDefault();
    const current = action;
    const file = selected;
    if (!current || busy) return;
    if (current === 'upload' && (!file || !confirmed || tooLarge)) return;
    if (!password) {
      operationError = null;
      passwordMissing = true;
      await tick();
      document.getElementById('sys-password')?.focus();
      return;
    }
    operationError = null;
    passwordMissing = false;
    busy = 'auth';
    let failed = false;
    try {
      const operation = current === 'backup' ? 'backup' : 'restore';
      await app.api(`/system/${operation}/authorize`, 'POST', { password }, 'json', request.signal);
      password = '';
      if (request.signal.aborted) return;
      busy = 'transfer';
      if (current === 'backup') {
        downloadBackup();
        toast(tr('systemDownloadRequested'));
      } else if (current === 'upload' && file) {
        const snapshot = new Blob([file], { type: 'application/octet-stream' });
        await app.api('/system/restore', 'POST', snapshot, 'json', request.signal);
        selected = null;
        if (fileInput) fileInput.value = '';
        toast(tr('systemStageReady'), { body: tr('admRestartToApply') });
        if (diagnostics) diagnostics = { ...diagnostics, restore_status: 'pending_restart' };
      } else {
        await app.api('/system/restore', 'DELETE', undefined, 'json', request.signal);
        toast(tr('systemDiscarded'));
      }
      if (current !== 'backup') void loadDiagnostics();
      busy = null;
      actionOpen = false;
    } catch (error) {
      failed = true;
      operationError = error;
      if (current !== 'backup' && busy === 'transfer') void loadDiagnostics();
    } finally {
      password = '';
      busy = null;
    }
    if (failed) {
      await tick();
      document.getElementById(passwordInvalid ? 'sys-password' : 'sys-op-error')?.focus();
    }
  }

  // ---------- Notifications ----------
  type DeliveryStatus = 'pending' | 'delivered' | 'failed' | 'discarded';
  interface Delivery {
    id: number; event: string; occurred_at: number; status: DeliveryStatus; attempts: number;
    last_error: string | null; next_attempt_at: number; delivered_at: number | null;
  }
  interface NotificationReport {
    enabled: boolean; origin: string | null; connector: boolean; events: string[];
    click_milestones: number[]; expiry_notice_hours: number | null;
    counts: Record<DeliveryStatus, number>; items: Delivery[];
  }
  const deliveryLabels: Record<DeliveryStatus, keyof Messages> = {
    pending: 'notificationPending', delivered: 'notificationDelivered', failed: 'notificationFailed', discarded: 'notificationDiscarded',
  };
  const deliveryErrors: Record<string, keyof Messages> = {
    blocked: 'monitorBlocked', busy: 'monitorBusy', dns: 'monitorDns', timeout: 'monitorTimeout',
    connection: 'monitorConnection', protocol: 'notificationRejected', restored: 'notificationRestored', stale: 'notificationStale',
  };
  let notifications = $state.raw<NotificationReport | null>(null);
  let notificationsError = $state<keyof Messages | null>(null);
  let notificationsLoading = $state(false);
  let sending = $state(false);

  async function loadNotifications() {
    notificationsLoading = true;
    notificationsError = null;
    try {
      notifications = await app.api<NotificationReport>('/notifications', 'GET', undefined, 'json', request.signal);
    } catch (error) {
      if (!request.signal.aborted) notificationsError = problemMessage(error);
    } finally {
      notificationsLoading = false;
    }
  }

  async function sendTest() {
    if (sending) return;
    sending = true;
    notificationsError = null;
    try {
      await app.api('/notifications/test', 'POST', undefined, 'json', request.signal);
      toast(tr('notificationTestQueued'), { tone: 'info' });
      await loadNotifications();
    } catch (error) {
      if (!request.signal.aborted) notificationsError = problemMessage(error);
    } finally {
      sending = false;
    }
  }

  function deliveryState(item: Delivery): StateView {
    const label = tr(deliveryLabels[item.status]);
    if (item.status === 'delivered') return { tone: 'success', icon: 'circle-check', label };
    if (item.status === 'failed') return { tone: 'error', icon: 'circle-x', label };
    if (item.status === 'discarded') return { tone: 'muted', icon: 'ban', label };
    return item.attempts > 0
      ? { tone: 'warning', icon: 'clock', label: tr('admRetrying', { count: item.attempts }) }
      : { tone: 'info', icon: 'clock', label };
  }

  // ---------- Background jobs ----------
  type LookupStatus = 'pending' | 'applied' | 'no_title' | 'failed' | 'skipped';
  type LookupFailure = 'blocked' | 'busy' | 'dns' | 'timeout' | 'connection' | 'protocol' | 'not_html' | 'policy' | 'expired' | 'restored';
  interface Lookup {
    link_id: number; host: string; slug: string; status: LookupStatus; failure: LookupFailure | null;
    title: string | null; requested_at: number; completed_at: number | null;
  }
  interface LookupReport { enabled: boolean; pending: number; items: Lookup[] }
  const lookupStates: Record<LookupStatus, [StateView['tone'], IconName, keyof Messages]> = {
    pending: ['info', 'clock', 'titlePending'], applied: ['success', 'circle-check', 'titleApplied'],
    no_title: ['muted', 'circle-dot', 'titleNone'], failed: ['error', 'circle-x', 'titleFailed'], skipped: ['muted', 'ban', 'titleSkipped'],
  };
  const lookupFailures: Record<LookupFailure, keyof Messages> = {
    blocked: 'monitorBlocked', busy: 'monitorBusy', dns: 'monitorDns', timeout: 'monitorTimeout',
    connection: 'monitorConnection', protocol: 'monitorProtocol', not_html: 'titleNotHtml',
    policy: 'titlePolicy', expired: 'titleExpired', restored: 'titleRestored',
  };
  let lookups = $state.raw<LookupReport | null>(null);
  let lookupsError = $state<keyof Messages | null>(null);
  let lookupsLoading = $state(false);
  let queueing = $state(false);
  let queued = $state<number | null>(null);

  async function loadLookups() {
    lookupsLoading = true;
    lookupsError = null;
    try {
      lookups = await app.api<LookupReport>('/title-lookups', 'GET', undefined, 'json', request.signal);
    } catch (error) {
      if (!request.signal.aborted) lookupsError = problemMessage(error);
    } finally {
      lookupsLoading = false;
    }
  }

  async function queueUntitled() {
    if (queueing) return;
    queueing = true;
    lookupsError = null;
    try {
      queued = (await app.api<{ queued: number }>('/title-lookups', 'POST', undefined, 'json', request.signal)).queued;
      toast(tr('titleQueued'), { body: num(queued) });
      await loadLookups();
    } catch (error) {
      if (!request.signal.aborted) lookupsError = error instanceof ApiError && error.code === 'title_lookup_disabled' ? 'titleDisabled' : problemMessage(error);
    } finally {
      queueing = false;
    }
  }

  // ---------- Audit log ----------
  const auditActions: Record<string, keyof Messages> = {
    setup_token_issued: 'auditSetupToken', recovery_token_issued: 'auditRecoveryToken', account_setup: 'auditSetup',
    login: 'auditLogin', account_recovery: 'auditRecovery', password_change: 'auditPassword',
    key_create: 'auditKeyCreate', key_rotate: 'auditKeyRotate', key_revoke: 'auditKeyRevoke',
    backup_authorize: 'auditBackupAuthorize', backup_download: 'auditBackupDownload',
    restore_authorize: 'auditRestoreAuthorize', restore_validate: 'auditRestoreValidate',
    restore_stage: 'auditRestoreStage', restore_cancel: 'auditRestoreCancel', restore_apply: 'auditRestoreApply',
    monitoring_update: 'auditMonitoringUpdate', destination_policy_update: 'auditDestinationPolicy',
  };
  const auditGroups = {
    account: ['setup_token_issued', 'recovery_token_issued', 'account_setup', 'login', 'account_recovery', 'password_change'],
    keys: ['key_create', 'key_rotate', 'key_revoke'],
    backups: ['backup_authorize', 'backup_download', 'restore_authorize', 'restore_validate', 'restore_stage', 'restore_cancel', 'restore_apply'],
    settings: ['monitoring_update', 'destination_policy_update'],
  } as const;
  type AuditGroup = keyof typeof auditGroups;
  const auditGroupLabels: Record<AuditGroup, keyof Messages> = {
    account: 'admAuditAccount', keys: 'apiKeys', backups: 'admAuditBackups', settings: 'admAuditSettings',
  };
  interface AuditItem {
    id: number; created_at: number; action: string; actor: 'admin' | 'system' | 'api_key';
    object_id: number | null; outcome: 'success' | 'rejected' | 'failed';
  }
  const actorLabels: Record<AuditItem['actor'], keyof Messages> = { admin: 'auditAdmin', system: 'auditSystem', api_key: 'apiKeys' };
  const outcomeStates: Record<AuditItem['outcome'], [StateView['tone'], IconName, keyof Messages]> = {
    success: ['success', 'check', 'auditSuccess'], rejected: ['warning', 'ban', 'auditRejected'], failed: ['error', 'circle-x', 'auditFailed'],
  };
  const initialGroup = route.params.get('action') as AuditGroup;
  let auditGroup = $state<AuditGroup | ''>(Object.hasOwn(auditGroups, initialGroup ?? '') ? initialGroup : '');
  let auditRows = $state<AuditItem[]>([]);
  let auditCursor = $state<number | null>(null);
  let auditLoading = $state(false);
  let auditLoaded = $state(false);
  let auditError = $state<keyof Messages | null>(null);
  const auditVisible = $derived(auditGroup ? auditRows.filter((row) => (auditGroups[auditGroup as AuditGroup] as readonly string[]).includes(row.action)) : auditRows);
  $effect(() => setParams({ action: tab === 'audit' ? auditGroup || null : null }));

  async function loadAudit(before?: number) {
    if (auditLoading) return;
    auditLoading = true;
    auditError = null;
    try {
      const pageResult = await app.api<{ items: AuditItem[]; next_before: number | null }>(before ? `/audit?before=${before}` : '/audit', 'GET', undefined, 'json', request.signal);
      auditRows = before ? [...auditRows, ...pageResult.items] : pageResult.items;
      auditCursor = pageResult.next_before;
      auditLoaded = true;
    } catch (error) {
      if (!request.signal.aborted) auditError = problemMessage(error);
    } finally {
      auditLoading = false;
    }
  }

  // Load each tab's data the first time it is shown.
  $effect(() => {
    if (tab === 'notifications' && !notifications && !notificationsLoading && !notificationsError) void loadNotifications();
    if (tab === 'jobs' && !lookups && !lookupsLoading && !lookupsError) void loadLookups();
    if (tab === 'audit' && !auditLoaded && !auditLoading && !auditError) void loadAudit();
  });

  function refresh() {
    if (tab === 'status' || tab === 'backups') void loadDiagnostics();
    else if (tab === 'notifications') void loadNotifications();
    else if (tab === 'jobs') void loadLookups();
    else void loadAudit();
  }
  const refreshBusy = $derived(
    tab === 'notifications' ? notificationsLoading : tab === 'jobs' ? lookupsLoading : tab === 'audit' ? auditLoading : refreshing,
  );

  function view(tone: StateView['tone'], icon: IconName, label: string): StateView {
    return { tone, icon, label };
  }

  function stamp(value: number) {
    return dateTime(value, false);
  }

  const storageTone = $derived(!diagnostics ? 'muted' : diagnostics.storage.status === 'healthy' ? 'success' : diagnostics.storage.status === 'low' ? 'warning' : 'muted');
  const storageFree = $derived(
    diagnostics?.storage.available_bytes != null && diagnostics.storage.total_bytes
      ? diagnostics.storage.available_bytes / diagnostics.storage.total_bytes
      : null,
  );
</script>

{#snippet failure(message: string, retry: () => void)}
  <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{message}</p></div><button class="btn" data-size="sm" onclick={retry}>{tr('tryAgain')}</button></div>
{/snippet}

<div class="page">
  <PageHeader title={tr('navSystem')} description={tr('admSystemDescription')}>
    {#snippet actions()}
      <button class="btn" disabled={refreshBusy || !!busy} aria-busy={refreshBusy} onclick={refresh}><Icon name="refresh-cw" />{tr('monitorRefresh')}</button>
    {/snippet}
  </PageHeader>

  <div class="tabs" role="tablist" aria-label={tr('admSystemSections')} tabindex="-1" onkeydown={tabKeys}>
    {#each TABS as [key, label, icon] (key)}
      <button id="sys-tab-{key}" class="tab" role="tab" aria-selected={tab === key} aria-controls="sys-panel" tabindex={tab === key ? 0 : -1} onclick={() => (tab = key)}>
        <Icon name={icon} />{tr(label)}
      </button>
    {/each}
  </div>

  <div id="sys-panel" class="stack" role="tabpanel" aria-labelledby="sys-tab-{tab}" aria-busy={refreshBusy}>
    {#if tab === 'status' || tab === 'backups'}
      {#if diagnosticsError}{@render failure(errorMessage(diagnosticsError), loadDiagnostics)}{/if}
      {#if !diagnostics && refreshing}<p class="muted" role="status">{tr('loadingPage')}</p>{/if}
    {/if}

    {#if tab === 'status' && diagnostics}
      {@const d = diagnostics}
      {#if d.storage.status === 'low'}
        <div class="notice" data-tone="warning" role="alert"><Icon name="triangle-alert" /><div><b>{tr('systemStorageLow')}</b><p>{tr('systemStorageLowHint', { threshold: bytes(d.storage.low_threshold_bytes) })}</p></div></div>
      {/if}
      <div class="tiles">
        <section class="panel tile">
          <span class="t-label" data-tone="success"><Icon name="database" />{tr('admDatabase')}</span>
          <b class="num">{bytes(d.database_bytes)}</b>
          <span class="muted">{tr('admDatabaseDetail', { schema: d.schema_version, sqlite: d.sqlite_version })}</span>
        </section>
        <section class="panel tile">
          <span class="t-label" data-tone={storageTone}><Icon name="hard-drive" />{tr('admStorage')}</span>
          <b class="num">{d.storage.available_bytes === null ? tr('systemUnknown') : tr('admFree', { value: bytes(d.storage.available_bytes) })}</b>
          {#if storageFree !== null}<div class="meter" data-tone={d.storage.status === 'low' ? 'warning' : undefined} aria-hidden="true"><span style:width="{Math.round(storageFree * 100)}%"></span></div>{/if}
          <span class="muted">
            {d.storage.status === 'healthy' ? tr('systemStorageHealthy') : d.storage.status === 'low' ? tr('systemStorageLow') : tr('systemUnknown')}{#if d.storage.total_bytes !== null}, {tr('admOfTotal', { total: bytes(d.storage.total_bytes) })}{/if}
          </span>
        </section>
        <section class="panel tile">
          <span class="t-label" data-tone={d.process_rss_bytes === null ? 'muted' : 'success'}><Icon name="cpu" />{tr('admMemoryCpu')}</span>
          <b class="num">{d.process_rss_bytes === null ? tr('systemUnknown') : bytes(d.process_rss_bytes)}</b>
          <span class="muted">{tr('admMemoryDetail', { container: containerMemory(d.container_memory), cpu: d.cpu.process_percent === null ? tr('systemUnknown') : new Intl.NumberFormat(locale.language, { style: 'percent', maximumFractionDigits: 1 }).format(d.cpu.process_percent / 100) })}</span>
        </section>
        <section class="panel tile">
          <span class="t-label" data-tone={d.analytics_enabled ? 'success' : 'muted'}><Icon name="chart-column" />{tr('analytics')}</span>
          <b class="num">{num(d.raw_event_count)}</b>
          <span class="muted">{tr('systemRawEventCount')}, {d.analytics_enabled ? tr('admCollectionOn') : tr('admCollectionOff')}</span>
        </section>
      </div>
      <div class="grid-2">
        <section class="panel" aria-labelledby="sys-build">
          <header class="panel-head"><div class="panel-title"><h2 id="sys-build">{tr('admBuild')}</h2></div></header>
          <dl class="facts panel-body">
            <dt>{tr('systemVersion')}</dt><dd>{d.version}</dd>
            <dt>{tr('systemBuildRevision')}</dt>
            <dd class="rev">
              {#if d.build_revision}<span class="mono">{d.build_revision.slice(0, 7)}</span><CopyButton value={d.build_revision} label={tr('admCopyRevision')} />{:else}{tr('systemUnknown')}{/if}
            </dd>
            <dt>{tr('systemStartedAt')}</dt><dd>{dateTime(d.started_at)}</dd>
            <dt>{tr('systemUptime')}</dt><dd>{uptime(d.uptime_seconds)}</dd>
            <dt>{tr('systemSqlite')}</dt><dd>{d.sqlite_version}</dd>
            <dt>{tr('systemSchema')}</dt><dd>{d.schema_version}</dd>
          </dl>
        </section>
        <section class="panel" aria-labelledby="sys-activity">
          <header class="panel-head"><div class="panel-title"><h2 id="sys-activity">{tr('admActivity')}</h2></div></header>
          <dl class="facts panel-body">
            <dt>{tr('systemLinkCount')}</dt><dd class="num">{num(d.link_count)}</dd>
            <dt>{tr('systemAnalyticsEnabled')}</dt><dd>{d.analytics_enabled ? tr('enabled') : tr('disabled')}</dd>
            <dt>{tr('systemRawEventCount')}</dt><dd class="num">{num(d.raw_event_count)}</dd>
            <dt>{tr('systemMonitoringEnabled')}</dt><dd>{d.monitoring_enabled ? tr('enabled') : tr('disabled')}</dd>
            <dt>{tr('systemLastMonitorCheck')}</dt><dd>{dateTime(d.last_monitor_check_at)}</dd>
            <dt>{tr('systemOperationStatus')}</dt><dd>{d.system_operation_in_progress ? tr('systemOperationActive') : tr('systemOperationIdle')}</dd>
          </dl>
        </section>
      </div>
      <section class="panel" aria-labelledby="sys-resources">
        <header class="panel-head"><div class="panel-title"><h2 id="sys-resources">{tr('systemResources')}</h2><p>{tr('systemResourcesHint')}</p></div></header>
        <dl class="facts panel-body resources">
          <dt>{tr('systemStorage')}</dt><dd>{storageValue(d.storage)}</dd>
          <dt>{tr('systemStorageStatus')}</dt><dd>{d.storage.status === 'healthy' ? tr('systemStorageHealthy') : d.storage.status === 'low' ? tr('systemStorageLow') : tr('systemUnknown')}</dd>
          <dt>{tr('systemCpuProcess')}</dt><dd>{cpu(d.cpu.process_percent, d.cpu.window_ms)}</dd>
          <dt>{tr('systemCpuContainer')}</dt><dd>{cpu(d.cpu.container_percent, d.cpu.window_ms)}</dd>
          <dt>{tr('systemCores')}</dt><dd>{d.cpu.available_cores === null ? tr('systemUnknown') : num(d.cpu.available_cores)}</dd>
          <dt>{tr('systemResidentMemory')}</dt><dd>{d.process_rss_bytes === null ? tr('systemUnknown') : bytes(d.process_rss_bytes)}</dd>
          <dt>{tr('systemContainerMemory')}</dt><dd>{containerMemory(d.container_memory)}</dd>
        </dl>
      </section>
    {:else if tab === 'backups' && diagnostics}
      {@const d = diagnostics}
      <div class="grid-main-side">
        <section class="panel" aria-labelledby="sys-backup">
          <header class="panel-head">
            <div class="panel-title"><h2 id="sys-backup">{tr('systemBackup')}</h2><p>{tr('systemBackupHint')}</p></div>
            <span class="spacer"></span>
            <button class="btn" data-variant="primary" disabled={d.system_operation_in_progress} onclick={() => choose('backup')}><Icon name="download" />{tr('systemDownload')}</button>
          </header>
          <dl class="facts panel-body">
            <dt>{tr('systemBackupStatus')}</dt>
            <dd>
              {#if d.last_backup_status === 'verified'}<Badge state={view('success', 'shield-check', tr('systemBackupVerified'))} />
              {:else if d.last_backup_status === 'failed'}<Badge state={view('error', 'circle-x', tr('systemBackupFailed'))} />
              {:else}<span class="muted">{tr('admNoBackupYet')}</span>{/if}
            </dd>
            <dt>{tr('systemLastVerifiedBackup')}</dt><dd>{d.last_verified_backup_at === null ? tr('instanceNoBackup') : dateTime(d.last_verified_backup_at)}</dd>
            <dt>{tr('systemDatabaseSize')}</dt><dd>{bytes(d.database_bytes)}</dd>
            <dt>{tr('systemOperationStatus')}</dt><dd>{d.system_operation_in_progress ? tr('systemOperationActive') : tr('systemOperationIdle')}</dd>
          </dl>
          {#if d.system_operation_in_progress}
            <div class="panel-body tight"><div class="notice" data-tone="info"><Icon name="info" /><div><p>{tr('systemSystemBusy')}</p></div></div></div>
          {/if}
        </section>
        <div class="stack">
          <section class="panel" aria-labelledby="sys-schedule">
            <header class="panel-head"><div class="panel-title"><h2 id="sys-schedule">{tr('admSchedule')}</h2></div></header>
            <dl class="facts panel-body">
              <dt>{tr('systemSchedule')}</dt><dd>{d.scheduled_backups ? tr('enabled') : tr('disabled')}</dd>
            </dl>
            <p class="field-hint schedule-hint">{tr('admScheduleHint1')} <code>BACKUP_INTERVAL_HOURS</code> {tr('admScheduleHint2')} <code>BACKUP_KEEP</code> {tr('admScheduleHint3')}</p>
          </section>
          <section class="panel danger-zone" aria-labelledby="sys-restore">
            <header class="panel-head"><div class="panel-title"><h2 id="sys-restore">{tr('systemRestore')}</h2><p>{tr('admRestoreSubtitle')}</p></div></header>
            <div class="panel-body restore">
              <dl class="facts"><dt>{tr('systemRestoreStatus')}</dt><dd>{tr(restoreLabels[d.restore_status])}</dd></dl>
              {#if pending}
                <div class="notice" data-tone="warning"><Icon name="triangle-alert" /><div><b>{tr('admRestoreStaged')}</b><p>{tr('systemRestartHint')}</p></div></div>
                <div><button class="btn" data-variant="ghost" onclick={() => choose('discard')}><Icon name="x" />{tr('systemDiscard')}</button></div>
              {:else}
                <p class="secondary small">{tr('systemRestoreHint')}</p>
                <div>
                  <button class="btn" data-variant="danger-quiet" disabled={d.restore_status === 'unavailable' || d.system_operation_in_progress} onclick={() => choose('upload')}>
                    <Icon name="rotate-ccw" />{tr('admRestoreOpen')}
                  </button>
                </div>
              {/if}
            </div>
          </section>
        </div>
      </div>
    {:else if tab === 'notifications'}
      {#if notificationsError}{@render failure(tr(notificationsError), loadNotifications)}{/if}
      {#if notifications}
        {@const n = notifications}
        <div class="grid-main-side">
          <section class="panel" aria-labelledby="sys-deliveries">
            <header class="panel-head">
              <div class="panel-title"><h2 id="sys-deliveries">{tr('admRecentDeliveries')}</h2><p>{tr('admDeliveriesHint')}</p></div>
              <span class="spacer"></span>
              {#if n.enabled}
                <button class="btn" data-size="sm" disabled={sending} aria-busy={sending} onclick={sendTest}><Icon name="bell" />{tr('notificationSendTest')}</button>
              {/if}
            </header>
            {#if n.counts.failed > 0}
              <div class="panel-body tight"><div class="notice" data-tone="error"><Icon name="circle-alert" /><div><p>{tr('notificationFailedHint')}</p></div></div></div>
            {/if}
            {#if n.items.length}
              <div class="table-wrap">
                <table class="table" data-density="compact">
                  <thead><tr><th scope="col">{tr('notificationEvent')}</th><th scope="col">{tr('admDelivery')}</th><th scope="col">{tr('notificationTime')}</th></tr></thead>
                  <tbody>
                    {#each n.items as item (item.id)}
                      <tr>
                        <td class="mono small">{item.event}</td>
                        <td>
                          <div class="delivery">
                            <Badge state={deliveryState(item)} />
                            {#if item.last_error}<span class="muted xs">{deliveryErrors[item.last_error] ? tr(deliveryErrors[item.last_error]) : item.last_error}</span>{/if}
                            {#if item.status === 'pending' && item.attempts > 0}<span class="muted xs">{tr('notificationNextAttempt')} {stamp(item.next_attempt_at)}</span>{/if}
                          </div>
                        </td>
                        <td class="muted nowrap"><time datetime={new Date((item.delivered_at ?? item.occurred_at) * 1000).toISOString()}>{stamp(item.delivered_at ?? item.occurred_at)}</time></td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <Empty icon="bell" title={tr('admNoDeliveries')} />
            {/if}
          </section>
          <div class="stack">
            <section class="panel" aria-labelledby="sys-receiver">
              <header class="panel-head"><div class="panel-title"><h2 id="sys-receiver">{tr('admReceiver')}</h2></div></header>
              {#if n.enabled}
                <dl class="facts panel-body">
                  <dt>{tr('notificationDestination')}</dt><dd class="mono">{n.origin}{n.connector ? ` · ${tr('notificationConnector')}` : ''}</dd>
                  <dt>{tr('notificationEvents')}</dt><dd>{n.events.join(', ')}</dd>
                  {#if n.click_milestones.length}<dt>{tr('notificationMilestones')}</dt><dd>{n.click_milestones.map((value) => num(value)).join(', ')}</dd>{/if}
                  {#if n.expiry_notice_hours !== null}<dt>{tr('notificationExpiryNotice')}</dt><dd>{new Intl.NumberFormat(locale.language, { style: 'unit', unit: 'hour', unitDisplay: 'long' }).format(n.expiry_notice_hours)}</dd>{/if}
                </dl>
              {:else}
                <div class="panel-body"><div class="notice" data-tone="neutral"><Icon name="webhook" /><div><p>{tr('notificationsOff')}</p></div></div></div>
              {/if}
            </section>
            <section class="panel" aria-labelledby="sys-outbox">
              <header class="panel-head"><div class="panel-title"><h2 id="sys-outbox">{tr('admOutbox')}</h2></div></header>
              <dl class="facts panel-body">
                {#each Object.entries(deliveryLabels) as [key, label] (key)}
                  <dt>{tr(label)}</dt><dd class="num">{num(n.counts[key as DeliveryStatus])}</dd>
                {/each}
              </dl>
            </section>
          </div>
        </div>
      {:else if notificationsLoading}
        <p class="muted" role="status">{tr('loadingPage')}</p>
      {/if}
    {:else if tab === 'jobs'}
      {#if lookupsError}{@render failure(tr(lookupsError), loadLookups)}{/if}
      {#if lookups}
        <section class="panel" aria-labelledby="sys-lookups">
          <header class="panel-head">
            <div class="panel-title"><h2 id="sys-lookups">{tr('titleLookups')}</h2><p>{tr('titleLookupsIntro')}</p></div>
            <span class="spacer"></span>
            {#if lookups.enabled}
              <span class="muted xs" role="status">{tr('titlePendingCount')} {num(lookups.pending)}{queued === null ? '' : ` · ${tr('titleQueued')} ${num(queued)}`}</span>
              <button class="btn" data-variant="primary" disabled={queueing} aria-busy={queueing} onclick={queueUntitled}>
                {#if queueing}<span class="spinner" aria-hidden="true"></span>{/if}{tr('titleQueueUntitled')}
              </button>
            {/if}
          </header>
          {#if !lookups.enabled}
            <div class="panel-body"><div class="notice" data-tone="neutral"><Icon name="info" /><div><p>{tr('titleDisabled')}</p></div></div></div>
          {/if}
          {#if lookups.items.length}
            <div class="table-wrap">
              <table class="table" data-density="compact">
                <thead><tr><th scope="col">{tr('admLink')}</th><th scope="col">{tr('admResult')}</th><th scope="col">{tr('notificationTime')}</th></tr></thead>
                <tbody>
                  {#each lookups.items as item (item.link_id)}
                    {@const [tone, icon, label] = lookupStates[item.status]}
                    <tr>
                      <td class="mono small">{item.host}/{item.slug}</td>
                      <td>
                        <div class="delivery">
                          <Badge state={view(tone, icon, tr(label))} />
                          {#if item.failure}<span class="muted xs">{tr(lookupFailures[item.failure])}</span>{/if}
                          {#if item.title}<span class="secondary xs">{item.title}</span>{/if}
                        </div>
                      </td>
                      <td class="muted nowrap"><time datetime={new Date((item.completed_at ?? item.requested_at) * 1000).toISOString()}>{stamp(item.completed_at ?? item.requested_at)}</time></td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {:else if lookups.enabled}
            <Empty icon="refresh-cw" title={tr('titleNoLookups')} />
          {/if}
        </section>
      {:else if lookupsLoading}
        <p class="muted" role="status">{tr('loadingPage')}</p>
      {/if}
    {:else if tab === 'audit'}
      {#if auditError}{@render failure(tr(auditError), () => loadAudit())}{/if}
      <section class="panel" aria-labelledby="sys-audit" aria-busy={auditLoading}>
        <header class="panel-head">
          <div class="panel-title"><h2 id="sys-audit">{tr('auditLog')}</h2><p>{tr('admAuditIntro')}</p></div>
          <span class="spacer"></span>
          <select class="select filter" data-size="sm" bind:value={auditGroup} aria-label={tr('admAuditFilter')} aria-describedby="sys-audit-hint">
            <option value="">{tr('admAuditAll')}</option>
            {#each Object.entries(auditGroupLabels) as [key, label] (key)}<option value={key}>{tr(label)}</option>{/each}
          </select>
        </header>
        {#if auditVisible.length}
          <div class="table-wrap">
            <table class="table" data-density="compact">
              <thead><tr><th scope="col">{tr('admWhen')}</th><th scope="col">{tr('admAction')}</th><th scope="col">{tr('admActor')}</th><th scope="col">{tr('admObject')}</th><th scope="col">{tr('admResult')}</th></tr></thead>
              <tbody>
                {#each auditVisible as item (item.id)}
                  {@const [tone, icon, label] = outcomeStates[item.outcome] ?? ['muted', 'circle-dot', 'unknown']}
                  <tr>
                    <td class="muted nowrap"><time datetime={new Date(item.created_at * 1000).toISOString()}>{stamp(item.created_at)}</time></td>
                    <td><b class="strong">{auditActions[item.action] ? tr(auditActions[item.action]) : item.action}</b></td>
                    <td>{actorLabels[item.actor] ? tr(actorLabels[item.actor]) : item.actor}</td>
                    <td class="secondary mono">{item.object_id === null ? '—' : `#${item.object_id}`}</td>
                    <td><Badge state={view(tone, icon, tr(label))} /></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else if auditLoading}
          <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
        {:else if auditLoaded}
          <Empty icon="file-text" title={auditRows.length ? tr('admAuditNoMatch') : tr('auditEmpty')} />
        {/if}
        <footer class="panel-foot">
          <span class="field-hint" id="sys-audit-hint">{tr('admAuditLoaded', { count: auditRows.length })}</span>
          <span class="spacer"></span>
          {#if auditCursor !== null}
            <button class="btn" data-size="sm" disabled={auditLoading} aria-busy={auditLoading} onclick={() => loadAudit(auditCursor ?? undefined)}>{tr('monitorMore')}</button>
          {/if}
        </footer>
      </section>
    {/if}
  </div>
</div>

<Dialog
  bind:open={actionOpen}
  title={action ? tr(actionTitles[action]) : tr('systemRestore')}
  description={action === 'upload' ? tr('admRestoreDescription') : action === 'discard' ? tr('systemDiscardHint') : tr('admAuthorizeHint')}
  width={520}
  busy={busy !== null}
  onclose={resetAction}
>
  <form id="sys-op" class="form" onsubmit={submitAction} novalidate aria-busy={busy !== null}>
    {#if action === 'upload'}
      <div class="notice" data-tone="warning">
        <Icon name="triangle-alert" />
        <div><b>{tr('admRestoreWarnTitle')}</b><p>{tr('admRestoreWarnText')}</p></div>
      </div>
      <div class="field">
        <span class="field-label" id="sys-file-label">{tr('systemFile')}</span>
        <input
          id="sys-file"
          class="visually-hidden"
          type="file"
          tabindex="-1"
          accept=".sqlite,.db,.sqlite3,application/octet-stream,application/vnd.sqlite3"
          bind:this={fileInput}
          onchange={(event) => (selected = event.currentTarget.files?.[0] ?? null)}
          disabled={!!busy}
        />
        <button type="button" class="drop" disabled={!!busy} aria-labelledby="sys-file-label sys-file-name" aria-describedby={tooLarge ? 'sys-file-size sys-file-error' : 'sys-file-size'} data-invalid={tooLarge || undefined} onclick={() => fileInput?.click()}>
          <Icon name="upload" />
          <span id="sys-file-name">{selected ? selected.name : tr('admChooseBackup')}</span>
          <small id="sys-file-size">{selected ? bytes(selected.size) + ' · ' : ''}{tr('systemMaxSize')} {diagnostics ? bytes(diagnostics.max_snapshot_bytes) : tr('systemUnknown')}</small>
        </button>
        {#if tooLarge}<p class="field-error" id="sys-file-error" role="alert"><Icon name="circle-alert" />{tr('systemSnapshotTooLarge')}</p>{/if}
      </div>
      <label class="check confirm"><input class="checkbox" type="checkbox" bind:checked={confirmed} disabled={!!busy} /><span>{tr('systemStageConfirm')}</span></label>
    {/if}
    <div class="field">
      <label class="field-label" for="sys-password">{tr('currentPassword')}</label>
      <input id="sys-password" class="input" type="password" name="current_password" autocomplete="current-password" bind:value={password} disabled={!!busy} aria-invalid={passwordInvalid ? 'true' : undefined} aria-describedby={passwordInvalid ? 'sys-op-error' : undefined} />
      {#if passwordInvalid}<p class="field-error" id="sys-op-error" role="alert"><Icon name="circle-alert" />{passwordMissing ? tr('admPasswordRequired') : errorMessage(operationError)}</p>{/if}
    </div>
    {#if operationError && !passwordInvalid}
      <div class="notice" data-tone="error" role="alert" id="sys-op-error" tabindex="-1"><Icon name="circle-alert" /><div><p>{errorMessage(operationError)}</p></div></div>
    {/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={!!busy} onclick={closeAction}>{tr('cancel')}</button>
    <button
      class="btn"
      type="submit"
      form="sys-op"
      data-variant={action === 'upload' ? 'danger' : 'primary'}
      disabled={!action || !!busy || (action === 'upload' && (!selected || !confirmed || tooLarge))}
      aria-busy={!!busy}
    >
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
      {busy === 'auth' ? tr('systemAuthorizing') : busy ? tr('systemProcessing') : action ? tr(actionLabels[action]) : tr('save')}
    </button>
  {/snippet}
</Dialog>

<style>
  .panel-foot > .spacer {
    flex: 1;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-4);
  }
  .tile {
    display: grid;
    gap: 6px;
    align-content: start;
    padding: var(--space-4) var(--space-5);
  }
  .t-label {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .t-label :global(svg) {
    width: 15px;
    height: 15px;
    color: var(--text-muted);
  }
  .t-label[data-tone='success'] :global(svg) {
    color: var(--success);
  }
  .t-label[data-tone='warning'] :global(svg) {
    color: var(--warning);
  }
  .tile b {
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
    font-weight: var(--weight-semibold);
  }
  .tile .muted {
    font-size: var(--text-xs);
  }
  .rev {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .resources {
    grid-template-columns: max-content minmax(0, 1fr) max-content minmax(0, 1fr);
  }
  .tight {
    padding-top: 0;
  }
  .schedule-hint {
    padding: 0 var(--space-5) var(--space-5);
  }
  .danger-zone {
    border-color: color-mix(in srgb, var(--error) 30%, var(--border-subtle));
  }
  .restore {
    display: grid;
    gap: var(--space-4);
  }
  .small {
    font-size: var(--text-sm);
  }
  .mono.small,
  .xs {
    font-size: var(--text-xs);
  }
  .nowrap {
    white-space: nowrap;
  }
  .strong {
    font-weight: var(--weight-medium);
  }
  .delivery {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
  }
  .filter {
    width: auto;
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  .confirm {
    font-size: var(--text-sm);
  }
  .drop {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 10px;
    align-items: center;
    width: 100%;
    padding: 12px 14px;
    border: 1px dashed var(--border-control);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-align: left;
  }
  .drop:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .drop[data-invalid] {
    border-color: var(--error);
  }
  .drop :global(svg) {
    grid-row: span 2;
    width: 18px;
    height: 18px;
    color: var(--text-muted);
  }
  .drop span {
    overflow-wrap: anywhere;
  }
  .drop small {
    color: var(--text-muted);
    font-family: var(--font-sans);
  }
  @media (max-width: 1100px) {
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .resources {
      grid-template-columns: max-content minmax(0, 1fr);
    }
  }
  @media (max-width: 560px) {
    .tiles {
      grid-template-columns: minmax(0, 1fr);
    }
    .resources {
      grid-template-columns: minmax(0, 1fr);
      gap: 2px;
    }
    .resources dd {
      margin-bottom: var(--space-2);
    }
  }
</style>
