<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from './api';
  import type { Api } from './api';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  interface ExchangeItem {
    host: string;
    slug: string;
    target: string;
    title: string;
    tags: string[];
    enabled: boolean;
    analytics_enabled: boolean;
  }

  interface ExchangeFile {
    version: 1;
    items: ExchangeItem[];
  }

  interface ExportPage extends ExchangeFile {
    next_after_id: number | null;
  }

  type FileFormat = 'json' | 'csv';

  interface DownloadPage {
    file: Blob;
    count: number | null;
    next_after_id: number | null;
  }

  interface ImportReport {
    version: 1;
    preview: boolean;
    applied: boolean;
    items: { host: string; slug: string; status: 'create' | 'unchanged' | 'conflict' }[];
  }

  let { t, language, api }: { t: Messages; language: Language; api: Api } = $props();

  const path = '/exchange/links';
  const maxFileBytes = 32 * 1024;
  const maxLinks = 100;
  const panelId = $props.id();

  let exportFormat = $state<FileFormat>('json');
  let page = $state.raw<DownloadPage | null>(null);
  let pageNumber = $state(1);
  let exportLoading = $state(false);
  let exportError = $state<keyof Messages | null>(null);
  let failedPage: { cursor: number | null; index: number } | null = null;

  let fileInput: HTMLInputElement;
  let importFile = $state.raw<ExchangeFile | Blob | null>(null);
  let report = $state.raw<ImportReport | null>(null);
  let reading = $state(false);
  let submitting = $state<'preview' | 'apply' | null>(null);
  let importError = $state<keyof Messages | null>(null);

  const busy = $derived(reading || submitting !== null);
  const canApply = $derived(
    importFile !== null && report?.preview === true && !report.applied
      && (importFile instanceof Blob || report.items.length === importFile.items.length)
      && report.items.every(({ status }) => status === 'create' || status === 'unchanged'),
  );
  const number = $derived(new Intl.NumberFormat(language));

  onMount(() => { void loadExport(null, 1); });

  async function loadExport(cursor: number | null, index: number) {
    if (exportLoading) return;
    exportLoading = true;
    exportError = null;
    failedPage = null;
    try {
      const query = cursor === null ? '' : `?after_id=${cursor}`;
      if (exportFormat === 'csv') {
        const response = await api<Response>(path + '.csv' + query, 'GET', undefined, 'csv');
        const rawCursor = response.headers.get('x-next-after-id');
        const cursor = rawCursor === null ? null : Number(rawCursor);
        if (cursor !== null && (!Number.isSafeInteger(cursor) || cursor < 1)) throw new Error();
        page = { file: await response.blob(), count: null, next_after_id: cursor };
      } else {
        const result = await api<ExportPage>(path + query);
        const file: ExchangeFile = { version: result.version, items: result.items };
        page = {
          file: new Blob([JSON.stringify(file) + '\n'], { type: 'application/json' }),
          count: result.items.length,
          next_after_id: result.next_after_id,
        };
      }
      pageNumber = index;
    } catch (error) {
      exportError = problemMessage(error);
      failedPage = { cursor, index };
    } finally {
      exportLoading = false;
    }
  }

  function changeExportFormat(event: Event & { currentTarget: HTMLSelectElement }) {
    exportFormat = event.currentTarget.value === 'csv' ? 'csv' : 'json';
    page = null;
    void loadExport(null, 1);
  }

  function retryExport() {
    if (failedPage) void loadExport(failedPage.cursor, failedPage.index);
  }

  function nextPage() {
    if (page && page.next_after_id !== null) void loadExport(page.next_after_id, pageNumber + 1);
  }

  function downloadPage() {
    if (!page || page.count === 0) return;
    const url = URL.createObjectURL(page.file);
    const download = document.createElement('a');
    download.href = url;
    download.download = `shortcircuit-links-v1-page-${pageNumber}.${exportFormat}`;
    download.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function resetImport() {
    if (busy) return;
    fileInput.value = '';
    importFile = null;
    report = null;
    importError = null;
  }

  async function chooseFile(event: Event & { currentTarget: HTMLInputElement }) {
    if (busy) return;
    const file = event.currentTarget.files?.[0];
    importFile = null;
    report = null;
    importError = null;
    if (!file) return;
    if (file.size > maxFileBytes) {
      importError = 'exchangeFileTooLarge';
      return;
    }

    if (file.name.toLowerCase().endsWith('.csv')) {
      importFile = new Blob([file], { type: 'text/csv' });
      return;
    }
    reading = true;
    try {
      const data: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await file.arrayBuffer()));
      if (!isExchangeFile(data)) {
        importError = 'exchangeInvalidFile';
        return;
      }
      const request = JSON.stringify({ ...data, preview: false });
      if (new TextEncoder().encode(request).byteLength > maxFileBytes) {
        importError = 'exchangeFileTooLarge';
        return;
      }
      importFile = data;
    } catch {
      importError = 'exchangeInvalidJson';
    } finally {
      reading = false;
    }
  }

  function isExchangeFile(value: unknown): value is ExchangeFile {
    if (typeof value !== 'object' || value === null) return false;
    const file = value as Record<string, unknown>;
    return Object.keys(file).length === 2 && file.version === 1
      && Array.isArray(file.items) && file.items.length >= 1 && file.items.length <= maxLinks
      && file.items.every(isExchangeItem);
  }

  function isExchangeItem(value: unknown): value is ExchangeItem {
    if (typeof value !== 'object' || value === null) return false;
    const item = value as Record<string, unknown>;
    return Object.keys(item).length === 7 && typeof item.host === 'string' && typeof item.slug === 'string'
      && typeof item.target === 'string' && typeof item.title === 'string'
      && typeof item.enabled === 'boolean' && typeof item.analytics_enabled === 'boolean'
      && Array.isArray(item.tags) && item.tags.every((tag: unknown) => typeof tag === 'string');
  }

  async function submitImport(preview: boolean) {
    if (!importFile || busy || (!preview && !canApply)) return;
    submitting = preview ? 'preview' : 'apply';
    importError = null;
    report = null;
    try {
      report = importFile instanceof Blob
        ? await api<ImportReport>(path + '.csv?preview=' + preview, 'POST', importFile)
        : await api<ImportReport>(path, 'POST', {
          version: importFile.version, preview, items: importFile.items,
        });
    } catch (error) {
      importError = !preview && error instanceof ApiError && error.status === 409
        ? 'exchangeApplyConflict'
        : problemMessage(error);
    } finally {
      submitting = null;
    }
  }

  function resultLabel(status: ImportReport['items'][number]['status'], applied: boolean): string {
    if (status === 'create') return applied ? t.exchangeCreated : t.exchangeWillCreate;
    if (status === 'unchanged') return t.exchangeUnchanged;
    return t.exchangeConflict;
  }
</script>

<div class="page-head"><h1>{t.exchange}</h1></div>

<div class="stack">
  <section class="panel panel-pad stack" aria-labelledby={panelId + '-export'} aria-busy={exportLoading}>
    <h2 id={panelId + '-export'}>{t.exchangeExport}</h2>
    <p class="muted">{t.exchangeExportScope}</p>
    <label class="field">
      <span id={panelId + '-export-format'}>{t.exchangeFormat}</span>
      <select value={exportFormat} onchange={changeExportFormat} disabled={exportLoading} aria-labelledby={panelId + '-export-format'}>
        <option value="json">JSON</option><option value="csv">CSV</option>
      </select>
    </label>
    {#if exportLoading}<p role="status">{t.loading}</p>{/if}
    {#if exportError}
      <div class="exchange-actions between">
        <p class="notice error" role="alert">{t[exportError]}</p>
        <button class="button" type="button" onclick={retryExport}>{t.retry}</button>
      </div>
    {/if}
    {#if page}
      <dl class="page-details">
        <div><dt>{t.exchangePage}</dt><dd>{number.format(pageNumber)}</dd></div>
        {#if page.count !== null}<div><dt>{t.exchangePageLinks}</dt><dd>{number.format(page.count)}</dd></div>{/if}
      </dl>
      {#if page.count === 0}<p>{t.exchangeNoLinks}</p>{/if}
      <div class="exchange-actions row">
        <button class="button primary" type="button" disabled={exportLoading || page.count === 0} onclick={downloadPage}>
          <Icon name="download" />{exportFormat === 'csv' ? t.exchangeDownloadCsv : t.exchangeDownloadPage}
        </button>
        {#if pageNumber > 1}
          <button class="button" type="button" disabled={exportLoading} onclick={() => loadExport(null, 1)}>{t.exchangeFirstPage}</button>
        {/if}
        <button class="button" type="button" disabled={exportLoading || page.next_after_id === null} onclick={nextPage}>{t.exchangeNextPage}</button>
      </div>
    {/if}
  </section>

  <section class="panel panel-pad stack" aria-labelledby={panelId + '-import'} aria-busy={busy}>
    <h2 id={panelId + '-import'}>{t.exchangeImport}</h2>
    <label class="field">
      <span id={panelId + '-file-label'}>{t.exchangeChooseFile}</span>
      <input type="file" accept=".json,.csv,application/json,text/csv" bind:this={fileInput} onchange={chooseFile} disabled={busy} aria-labelledby={panelId + '-file-label'} aria-describedby={panelId + '-file-hint'} />
      <small id={panelId + '-file-hint'}>{t.exchangeFileHint}</small>
    </label>
    <div class="exchange-actions row">
      <button class="button" type="button" disabled={busy || !importFile} onclick={() => submitImport(true)}>{t.exchangePreview}</button>
      <button class="button primary" type="button" disabled={busy || !canApply} onclick={() => submitImport(false)}>{t.exchangeApply}</button>
      <button class="button" type="button" disabled={busy || (!importFile && !importError)} onclick={resetImport}>{t.exchangeReset}</button>
    </div>
    {#if busy}<p role="status">{submitting === 'apply' ? t.exchangeApplying : t.loading}</p>{/if}
    {#if importError}<p class="notice error" role="alert">{t[importError]}</p>{/if}

    {#if report}
      <h3 id={panelId + '-report'}>{report.applied ? t.exchangeApplyReport : t.exchangePreviewReport}</h3>
      {#if report.applied}
        <p class="notice" role="status">{t.exchangeApplied}</p>
      {:else if report.items.some(({ status }) => status === 'conflict')}
        <p class="notice error" role="status">{t.exchangePreviewConflict}</p>
      {/if}
      <div class="table-wrap report-table">
        <table aria-labelledby={panelId + '-report'}>
          <thead><tr><th scope="col">{t.shortLink}</th><th scope="col">{t.status}</th></tr></thead>
          <tbody>
            {#each report.items as item}
              <tr>
                <th scope="row" class="mono">{item.host}/{item.slug}</th>
                <td>{resultLabel(item.status, report.applied)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
</div>

<style>
  .exchange-actions { flex-wrap: wrap; }

  .page-details {
    display: flex;
    flex-wrap: wrap;
    gap: 12px 24px;
    margin: 0;
  }

  dt { color: var(--sub); }
  dd { margin: 2px 0 0; font-weight: 650; font-variant-numeric: tabular-nums; }
  input[type='file'] { max-inline-size: 45rem; }
  .report-table th:first-child { width: 68%; }
  .report-table tbody th { background: transparent; }
</style>
