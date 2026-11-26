<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from './api';
  import type { Api } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import { messages } from './i18n';
  import type { Language } from './i18n';

  const statusLabels = {
    none: 'systemRestoreNone',
    pending_restart: 'systemRestorePending',
    applied: 'systemRestoreApplied',
    failed: 'systemRestoreFailed',
    interrupted: 'systemRestoreInterrupted',
    unavailable: 'systemRestoreUnavailable',
  } as const;

  const problemLabels = {
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
  } as const;

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
    restore_status: keyof typeof statusLabels;
    max_snapshot_bytes: number;
  }

  const actionLabels = { backup: 'systemDownload', upload: 'systemStage', discard: 'systemDiscard' } as const;
  type Action = keyof typeof actionLabels;

  let { api, language }: { api: Api; language: Language } = $props();
  const t = $derived(messages[language]);
  const ids = $props.id();
  const request = new AbortController();
  let diagnostics = $state.raw<Diagnostics | null>(null);
  let diagnosticsError = $state.raw<unknown>(null);
  let refreshing = $state(false);
  let action = $state<Action | null>(null);
  let busy = $state<'auth' | 'transfer' | null>(null);
  let password = $state('');
  let confirmed = $state(false);
  let selected = $state.raw<File | null>(null);
  let input = $state<HTMLInputElement>();
  let operationError = $state.raw<unknown>(null);
  let notice = $state<'systemDownloadRequested' | 'systemStageReady' | 'systemDiscarded' | null>(null);
  const pending = $derived(diagnostics?.restore_status === 'pending_restart');
  const tooLarge = $derived(!!selected && !!diagnostics && selected.size > diagnostics.max_snapshot_bytes);

  onMount(() => {
    void load();
    return () => request.abort();
  });

  async function load() {
    if (refreshing) return;
    refreshing = true;
    diagnosticsError = null;
    try {
      diagnostics = await api<Diagnostics>('/system', 'GET', undefined, 'json', request.signal);
    } catch (error) {
      diagnosticsError = error;
    } finally {
      refreshing = false;
    }
  }

  function bytes(value: number) {
    const unit = value >= 1024 ** 3 ? 'GiB' : 'MiB';
    const scale = unit === 'GiB' ? 1024 ** 3 : 1024 ** 2;
    return `${new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(value / scale)} ${unit}`;
  }

  function dateTime(value: number | null) {
    if (value === null) return t.systemUnknown;
    const time = new Date(value * 1000);
    if (!Number.isFinite(time.getTime())) return t.systemUnknown;
    return new Intl.DateTimeFormat(language, {
      year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
      second: '2-digit', timeZone: 'UTC', timeZoneName: 'short',
    }).format(time);
  }

  function uptime(seconds: number) {
    const [value, unit] = seconds >= 3600 ? [seconds / 3600, 'hour']
      : seconds >= 60 ? [seconds / 60, 'minute'] : [seconds, 'second'];
    return new Intl.NumberFormat(language, { style: 'unit', unit, maximumFractionDigits: 1 }).format(value);
  }

  function errorMessage(error: unknown) {
    if (error instanceof ApiError && Object.hasOwn(problemLabels, error.code)) {
      return t[problemLabels[error.code as keyof typeof problemLabels]];
    }
    return t[problemMessage(error)];
  }

  function choose(next: Action) {
    password = '';
    confirmed = false;
    operationError = null;
    notice = null;
    action = next;
  }

  function close() {
    if (busy) return;
    password = '';
    confirmed = false;
    operationError = null;
    action = null;
  }

  function downloadBackup() {
    const anchor = document.createElement('a');
    anchor.href = '/api/v1/system/backup';
    anchor.download = '';
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const current = action;
    const file = selected;
    if (!current || busy || (current === 'upload' && (!file || !confirmed || tooLarge))) return;
    operationError = null;
    busy = 'auth';
    try {
      const operation = current === 'backup' ? 'backup' : 'restore';
      await api(`/system/${operation}/authorize`, 'POST', { password }, 'json', request.signal);
      password = '';
      if (request.signal.aborted) return;
      busy = 'transfer';
      if (current === 'backup') {
        downloadBackup();
        notice = 'systemDownloadRequested';
      } else if (current === 'upload' && file) {
        const snapshot = new Blob([file], { type: 'application/octet-stream' });
        await api('/system/restore', 'POST', snapshot, 'json', request.signal);
        selected = null;
        if (input) input.value = '';
        notice = 'systemStageReady';
        if (diagnostics) diagnostics = { ...diagnostics, restore_status: 'pending_restart' };
      } else {
        await api('/system/restore', 'DELETE', undefined, 'json', request.signal);
        notice = 'systemDiscarded';
      }
      if (current !== 'backup') void load();
      action = null;
    } catch (error) {
      operationError = error;
      if (current !== 'backup' && busy === 'transfer') void load();
    } finally {
      password = '';
      busy = null;
    }
  }
</script>

<div class="page-head">
  <h1>{t.operations}</h1>
  <button class="button" type="button" onclick={load} disabled={refreshing || !!busy}>{t.systemRefresh}</button>
</div>

{#if notice}<p class="notice operations-notice" role="status">{t[notice]}</p>{/if}

<div class="operations-grid">
  <section class="panel panel-pad stack diagnostics-panel" aria-labelledby={ids + '-diagnostics'} aria-busy={refreshing}>
    <h2 id={ids + '-diagnostics'}>{t.systemDiagnostics}</h2>
    {#if diagnosticsError}<p class="notice error" role="alert">{errorMessage(diagnosticsError)}</p>{/if}
    {#if diagnostics}
      {#snippet metric(label: string, value: string | number)}
        <div><dt>{label}</dt><dd>{value}</dd></div>
      {/snippet}
      <dl class="diagnostics">
        {@render metric(t.systemVersion, diagnostics.version)}
        {@render metric(t.systemBuildRevision, diagnostics.build_revision ?? t.systemUnknown)}
        {@render metric(t.systemStartedAt, dateTime(diagnostics.started_at))}
        {@render metric(t.systemUptime, uptime(diagnostics.uptime_seconds))}
        {@render metric(t.systemResidentMemory, diagnostics.process_rss_bytes === null ? t.systemUnknown : bytes(diagnostics.process_rss_bytes))}
        {@render metric(t.systemSqlite, diagnostics.sqlite_version)}
        {@render metric(t.systemSchema, diagnostics.schema_version)}
        {@render metric(t.systemDatabaseSize, bytes(diagnostics.database_bytes))}
        {@render metric(t.systemLinkCount, diagnostics.link_count.toLocaleString(language))}
        {@render metric(t.systemRawEventCount, diagnostics.raw_event_count.toLocaleString(language))}
        {@render metric(t.systemAnalyticsEnabled, diagnostics.analytics_enabled ? t.enabled : t.disabled)}
        {@render metric(t.systemSchedule, diagnostics.scheduled_backups ? t.enabled : t.disabled)}
        {@render metric(t.systemBackupStatus,
          diagnostics.last_backup_status === 'verified' ? t.systemBackupVerified
            : diagnostics.last_backup_status === 'failed' ? t.systemBackupFailed : t.systemUnknown)}
        {@render metric(t.systemLastVerifiedBackup, dateTime(diagnostics.last_verified_backup_at))}
        {@render metric(t.systemMonitoringEnabled, diagnostics.monitoring_enabled ? t.enabled : t.disabled)}
        {@render metric(t.systemLastMonitorCheck, dateTime(diagnostics.last_monitor_check_at))}
        {@render metric(t.systemOperationStatus, diagnostics.system_operation_in_progress ? t.systemOperationActive : t.systemOperationIdle)}
        {@render metric(t.systemRestoreStatus, t[statusLabels[diagnostics.restore_status]])}
      </dl>
    {:else if refreshing}
      <p role="status">{t.loading}</p>
    {/if}
  </section>

  <section class="panel panel-pad stack" aria-labelledby={ids + '-backup'}>
    <h2 id={ids + '-backup'}>{t.systemBackup}</h2>
    <p>{t.systemBackupHint}</p>
    <div><button class="button" type="button" onclick={() => choose('backup')}>{t.systemDownload}</button></div>
  </section>

  <section class="panel panel-pad stack" aria-labelledby={ids + '-restore'}>
    <h2 id={ids + '-restore'}>{t.systemRestore}</h2>
    {#if pending}
      <p class="notice">{t.systemRestartHint}</p>
      <div><button class="button" type="button" onclick={() => choose('discard')}>{t.systemDiscard}</button></div>
    {:else}
      <p>{t.systemRestoreHint}</p>
      <label class="field">
        <span>{t.systemFile}</span>
        <input type="file" accept=".sqlite,.db,.sqlite3,application/octet-stream,application/vnd.sqlite3" bind:this={input}
          onchange={(event) => selected = event.currentTarget.files?.[0] ?? null}
          aria-describedby={diagnostics ? ids + '-size' : undefined} aria-invalid={tooLarge} />
        {#if diagnostics}<small id={ids + '-size'}>{t.systemMaxSize} {bytes(diagnostics.max_snapshot_bytes)}</small>{/if}
      </label>
      {#if tooLarge}<p class="notice error" role="alert">{t.systemSnapshotTooLarge}</p>{/if}
      <div><button class="button" type="button" disabled={!diagnostics || !selected || tooLarge || diagnostics.restore_status === 'unavailable'} onclick={() => choose('upload')}>{t.systemStage}</button></div>
    {/if}
  </section>
</div>

<Dialog {t} open={action !== null} title={action ? t[actionLabels[action]] : t.systemRestore} busy={busy !== null} onclose={close}>
  <form class="dialog-body stack" onsubmit={submit} aria-busy={busy !== null}>
    {#if action === 'upload'}
      <label class="confirmation"><input type="checkbox" required bind:checked={confirmed} disabled={!!busy} /> <span>{t.systemStageConfirm}</span></label>
    {:else if action === 'discard'}
      <p>{t.systemDiscardHint}</p>
    {/if}
    <label class="field">
      <span>{t.currentPassword}</span>
      <input type="password" required name="current_password" autocomplete="current-password" bind:value={password} disabled={!!busy}
        aria-invalid={operationError instanceof ApiError && operationError.code === 'invalid_current_password'} />
    </label>
    {#if operationError}<p class="notice error" role="alert">{errorMessage(operationError)}</p>{/if}
    <div class="row operation-actions">
      <button class="button" type="button" disabled={!!busy} onclick={close}>{t.cancel}</button>
      <button class="button primary" type="submit" disabled={!action || !!busy || (action === 'upload' && !confirmed)}>
        {busy === 'auth' ? t.systemAuthorizing : busy ? t.systemProcessing : action ? t[actionLabels[action]] : t.save}
      </button>
    </div>
  </form>
</Dialog>

<style>
  .operations-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr)); align-items: start; gap: 16px; }
  .diagnostics-panel { grid-column: 1 / -1; }
  .operations-notice { margin-bottom: 16px; }
  .diagnostics { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap: 10px 28px; margin: 0; }
  .diagnostics div { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .diagnostics dt { color: var(--muted); }
  .diagnostics dd { margin: 0; min-width: 0; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .confirmation { display: grid; grid-template-columns: auto 1fr; align-items: start; gap: 10px; }
  .confirmation input { margin-top: 4px; }
  .operation-actions { justify-content: flex-end; flex-wrap: wrap; }
</style>
