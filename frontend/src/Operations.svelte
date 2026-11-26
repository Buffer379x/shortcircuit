<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from './api';
  import type { Api } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import { messages } from './i18n';
  import type { Language } from './i18n';
  import { operationsMessages } from './operations-i18n';

  const statusLabels = {
    none: 'restoreNone',
    pending_restart: 'restorePending',
    applied: 'restoreApplied',
    failed: 'restoreFailed',
    interrupted: 'restoreInterrupted',
    unavailable: 'restoreUnavailable',
  } as const;

  const problemLabels = {
    invalid_current_password: 'incorrectPassword',
    operation_authorization_required: 'authorizationRequired',
    restore_already_pending: 'restoreAlreadyPending',
    invalid_snapshot: 'invalidSnapshot',
    no_pending_restore: 'noPendingRestore',
    snapshot_too_large: 'snapshotTooLarge',
    system_operation_in_progress: 'systemBusy',
    invalid_snapshot_upload: 'snapshotUploadFailed',
    snapshot_upload_timed_out: 'snapshotUploadFailed',
    unsupported_snapshot_media_type: 'snapshotUploadFailed',
  } as const;

  interface Diagnostics {
    version: string;
    sqlite_version: string;
    schema_version: number;
    database_bytes: number;
    scheduled_backups: boolean;
    restore_status: keyof typeof statusLabels;
    max_snapshot_bytes: number;
  }

  const actionLabels = { backup: 'download', upload: 'stage', discard: 'discard' } as const;
  type Action = keyof typeof actionLabels;

  let { api, language }: { api: Api; language: Language } = $props();
  const t = $derived(messages[language]);
  const text = $derived(operationsMessages[language]);
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
  let notice = $state<'downloadRequested' | 'stageReady' | 'discarded' | null>(null);
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

  function errorMessage(error: unknown) {
    if (error instanceof ApiError && Object.hasOwn(problemLabels, error.code)) {
      return text[problemLabels[error.code as keyof typeof problemLabels]];
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
        notice = 'downloadRequested';
      } else if (current === 'upload' && file) {
        const snapshot = new Blob([file], { type: 'application/octet-stream' });
        await api('/system/restore', 'POST', snapshot, 'json', request.signal);
        selected = null;
        if (input) input.value = '';
        notice = 'stageReady';
        if (diagnostics) diagnostics = { ...diagnostics, restore_status: 'pending_restart' };
      } else {
        await api('/system/restore', 'DELETE', undefined, 'json', request.signal);
        notice = 'discarded';
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
  <button class="button" type="button" onclick={load} disabled={refreshing || !!busy}>{text.refresh}</button>
</div>

{#if notice}<p class="notice operations-notice" role="status">{text[notice]}</p>{/if}

<div class="operations-grid">
  <section class="panel panel-pad stack" aria-labelledby={ids + '-diagnostics'} aria-busy={refreshing}>
    <h2 id={ids + '-diagnostics'}>{text.diagnostics}</h2>
    {#if diagnosticsError}<p class="notice error" role="alert">{errorMessage(diagnosticsError)}</p>{/if}
    {#if diagnostics}
      <dl class="diagnostics">
        <div><dt>{text.version}</dt><dd>{diagnostics.version}</dd></div>
        <div><dt>{text.sqlite}</dt><dd>{diagnostics.sqlite_version}</dd></div>
        <div><dt>{text.schema}</dt><dd>{diagnostics.schema_version}</dd></div>
        <div><dt>{text.databaseSize}</dt><dd>{bytes(diagnostics.database_bytes)}</dd></div>
        <div><dt>{text.schedule}</dt><dd>{diagnostics.scheduled_backups ? t.enabled : t.disabled}</dd></div>
        <div><dt>{text.restoreStatus}</dt><dd>{text[statusLabels[diagnostics.restore_status]]}</dd></div>
      </dl>
    {:else if refreshing}
      <p role="status">{t.loading}</p>
    {/if}
  </section>

  <section class="panel panel-pad stack" aria-labelledby={ids + '-backup'}>
    <h2 id={ids + '-backup'}>{text.backup}</h2>
    <p>{text.backupHint}</p>
    <div><button class="button" type="button" onclick={() => choose('backup')}>{text.download}</button></div>
  </section>

  <section class="panel panel-pad stack" aria-labelledby={ids + '-restore'}>
    <h2 id={ids + '-restore'}>{text.restore}</h2>
    {#if pending}
      <p class="notice">{text.restartHint}</p>
      <div><button class="button" type="button" onclick={() => choose('discard')}>{text.discard}</button></div>
    {:else}
      <p>{text.restoreHint}</p>
      <label class="field">
        <span>{text.file}</span>
        <input type="file" accept=".sqlite,.db,.sqlite3,application/octet-stream,application/vnd.sqlite3" bind:this={input}
          onchange={(event) => selected = event.currentTarget.files?.[0] ?? null}
          aria-describedby={diagnostics ? ids + '-size' : undefined} aria-invalid={tooLarge} />
        {#if diagnostics}<small id={ids + '-size'}>{text.maxSize} {bytes(diagnostics.max_snapshot_bytes)}</small>{/if}
      </label>
      {#if tooLarge}<p class="notice error" role="alert">{text.snapshotTooLarge}</p>{/if}
      <div><button class="button" type="button" disabled={!diagnostics || !selected || tooLarge || diagnostics.restore_status === 'unavailable'} onclick={() => choose('upload')}>{text.stage}</button></div>
    {/if}
  </section>
</div>

<Dialog {t} open={action !== null} title={action ? text[actionLabels[action]] : text.restore} busy={busy !== null} onclose={close}>
  <form class="dialog-body stack" onsubmit={submit} aria-busy={busy !== null}>
    {#if action === 'upload'}
      <label class="confirmation"><input type="checkbox" required bind:checked={confirmed} disabled={!!busy} /> <span>{text.stageConfirm}</span></label>
    {:else if action === 'discard'}
      <p>{text.discardHint}</p>
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
        {busy === 'auth' ? text.authorizing : busy ? text.processing : action ? text[actionLabels[action]] : t.save}
      </button>
    </div>
  </form>
</Dialog>

<style>
  .operations-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr)); align-items: start; gap: 16px; }
  .operations-notice { margin-bottom: 16px; }
  .diagnostics { display: grid; gap: 10px; margin: 0; }
  .diagnostics div { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .diagnostics dt { color: var(--muted); }
  .diagnostics dd { margin: 0; min-width: 0; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .confirmation { display: grid; grid-template-columns: auto 1fr; align-items: start; gap: 10px; }
  .confirmation input { margin-top: 4px; }
  .operation-actions { justify-content: flex-end; flex-wrap: wrap; }
</style>
