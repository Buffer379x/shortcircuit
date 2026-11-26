<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';

  const actions: Record<string, keyof Messages> = {
    setup_token_issued: 'auditSetupToken', recovery_token_issued: 'auditRecoveryToken', account_setup: 'auditSetup',
    login: 'auditLogin', account_recovery: 'auditRecovery', password_change: 'auditPassword',
    key_create: 'auditKeyCreate', key_rotate: 'auditKeyRotate', key_revoke: 'auditKeyRevoke',
    backup_authorize: 'auditBackupAuthorize', backup_download: 'auditBackupDownload',
    restore_authorize: 'auditRestoreAuthorize', restore_validate: 'auditRestoreValidate',
    restore_stage: 'auditRestoreStage', restore_cancel: 'auditRestoreCancel', restore_apply: 'auditRestoreApply',
    monitoring_update: 'auditMonitoringUpdate',
  };
  interface AuditItem {
    id: number;
    created_at: number;
    action: string;
    actor: 'admin' | 'system' | 'api_key';
    object_id: number | null;
    outcome: 'success' | 'rejected' | 'failed';
  }
  interface Page { items: AuditItem[]; next_before: number | null }

  let { api, language, t }: { api: Api; language: Language; t: Messages } = $props();
  let rows = $state<AuditItem[]>([]);
  let cursor = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<keyof Messages | null>(null);
  const time = $derived(new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short' }));
  const actors = $derived({ admin: t.auditAdmin, system: t.auditSystem, api_key: t.apiKeys });
  const outcomes = $derived({ success: t.auditSuccess, rejected: t.auditRejected, failed: t.auditFailed });

  onMount(() => { void load(); });

  async function load(before?: number) {
    if (loading) return;
    loading = true;
    error = null;
    try {
      const page = await api<Page>(before ? `/audit?before=${before}` : '/audit');
      rows = before ? [...rows, ...page.items] : page.items;
      cursor = page.next_before;
    } catch (issue) {
      error = problemMessage(issue);
    } finally {
      loading = false;
    }
  }
</script>

<header class="page-header">
  <div><h1>{t.auditLog}</h1><p>{t.auditIntro}</p></div>
  <button class="button" type="button" disabled={loading} onclick={() => load()}>{t.monitorRefresh}</button>
</header>
{#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
{#if loading && !rows.length}
  <p role="status">{t.loading}</p>
{:else if rows.length}
  <section class="panel panel-pad stack">
    {#each rows as item (item.id)}
      <article class="audit-event">
        <strong>{actions[item.action] ? t[actions[item.action]] : item.action}</strong>
        <time datetime={new Date(item.created_at * 1000).toISOString()}>{time.format(new Date(item.created_at * 1000))}</time>
        <span>{actors[item.actor]} · {outcomes[item.outcome]}{item.object_id === null ? '' : ` · #${item.object_id}`}</span>
      </article>
    {/each}
  </section>
  {#if cursor !== null}
    <p><button class="button" type="button" disabled={loading} onclick={() => load(cursor ?? undefined)}>{t.monitorMore}</button></p>
  {/if}
{:else if !error}
  <p>{t.auditEmpty}</p>
{/if}

<style>
  .audit-event { display: grid; gap: .35rem; padding-block: .65rem; border-bottom: 1px solid var(--border); overflow-wrap: anywhere; }
  .audit-event:last-child { border-bottom: 0; }
  @media (min-width: 720px) { .audit-event { grid-template-columns: 1.2fr 1fr 1fr; } }
</style>
