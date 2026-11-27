<script lang="ts">
  // Import (JSON or mapped CSV, previewed, then applied in one transaction) and paged export.
  import { onMount, tick } from 'svelte';
  import { ApiError } from '../api';
  import { downloadFile, problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import { app } from '../lib/app.svelte';
  import { num } from '../lib/fmt';
  import { locale, tr } from '../lib/locale.svelte';
  import type { StateView } from '../lib/status';
  import { toast } from '../lib/toast.svelte';

  interface ExchangeItem {
    host: string;
    slug: string;
    target: string;
    title: string;
    description?: string;
    tags: string[];
    enabled: boolean;
    analytics_enabled: boolean;
  }
  interface ExchangeFile { version: 1; items: ExchangeItem[] }
  interface ExportPage extends ExchangeFile { next_after_id: number | null }
  type FileFormat = 'json' | 'csv';
  interface DownloadPage { file: Blob; count: number | null; next_after_id: number | null }
  type ItemStatus = 'create' | 'unchanged' | 'conflict';
  interface ImportReport {
    version: 1;
    preview: boolean;
    applied: boolean;
    items: { host: string; slug: string; status: ItemStatus }[];
  }

  const csvFields = ['target', 'slug', 'host', 'title', 'description', 'tags', 'enabled', 'analytics_enabled'] as const;
  type CsvField = typeof csvFields[number];
  type ColumnSelection = Partial<Record<CsvField, number>>;
  const fieldLabels: Record<CsvField, keyof Messages> = {
    target: 'admFieldTarget', slug: 'exchangeCsvSlug', host: 'domain', title: 'admFieldTitle',
    description: 'admFieldDescription', tags: 'tags', enabled: 'enabled', analytics_enabled: 'analyticsEnabled',
  };

  const path = '/exchange/links';
  const maxFileBytes = 32 * 1024;
  const maxLinks = 100;
  const STEPS: (keyof Messages)[] = ['admStepChoose', 'admStepMap', 'admStepReview', 'admStepDone'];

  // ---------- Export ----------
  let exportFormat = $state<FileFormat>('json');
  let page = $state.raw<DownloadPage | null>(null);
  let pageNumber = $state(1);
  let exportLoading = $state(false);
  let exportError = $state<keyof Messages | null>(null);
  let failedPage: { cursor: number | null; index: number } | null = null;

  // ---------- Import ----------
  let step = $state<0 | 1 | 2 | 3>(0);
  let source = $state<'csv' | 'json'>('csv');
  let fileInput = $state<HTMLInputElement>();
  let fileName = $state('');
  let importFile = $state.raw<ExchangeFile | Blob | null>(null);
  let csvColumns = $state.raw<string[] | null>(null);
  let csvSelection = $state<ColumnSelection>({});
  let chosenHost = $state('');
  let report = $state.raw<ImportReport | null>(null);
  let reading = $state(false);
  let submitting = $state<'preview' | 'apply' | null>(null);
  let importError = $state<keyof Messages | null>(null);
  let dragging = $state(false);
  let errorBox = $state<HTMLDivElement>();
  let importHeading = $state<HTMLHeadingElement>();

  const busy = $derived(reading || submitting !== null);
  const selectedColumns = $derived(Object.values(csvSelection));
  const importHost = $derived(chosenHost || (app.domains?.hosts.length === 1 ? app.domains.hosts[0] : ''));
  const mappingReady = $derived(
    csvColumns === null || (csvSelection.target !== undefined && csvSelection.slug !== undefined
      && (csvSelection.host !== undefined || !!importHost)
      && new Set(selectedColumns).size === selectedColumns.length),
  );
  const canApply = $derived(
    importFile !== null && mappingReady && report?.preview === true && !report.applied
      && (importFile instanceof Blob || report.items.length === importFile.items.length)
      && report.items.every(({ status }) => status === 'create' || status === 'unchanged'),
  );
  const counts = $derived({
    create: report?.items.filter((item) => item.status === 'create').length ?? 0,
    conflict: report?.items.filter((item) => item.status === 'conflict').length ?? 0,
    unchanged: report?.items.filter((item) => item.status === 'unchanged').length ?? 0,
  });
  // One pink action per view: the import step owns it while an import is in progress.
  const exportPrimary = $derived(step === 0 || step === 3);

  onMount(() => { void loadExport(null, 1); });

  async function loadExport(cursor: number | null, index: number) {
    if (exportLoading) return;
    exportLoading = true;
    exportError = null;
    failedPage = null;
    try {
      const query = cursor === null ? '' : `?after_id=${cursor}`;
      if (exportFormat === 'csv') {
        const response = await app.api<Response>(path + '.csv' + query, 'GET', undefined, 'csv');
        const rawCursor = response.headers.get('x-next-after-id');
        const next = rawCursor === null ? null : Number(rawCursor);
        if (next !== null && (!Number.isSafeInteger(next) || next < 1)) throw new Error();
        page = { file: await response.blob(), count: null, next_after_id: next };
      } else {
        const result = await app.api<ExportPage>(path + query);
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

  function changeExportFormat(format: FileFormat) {
    if (format === exportFormat || exportLoading) return;
    exportFormat = format;
    page = null;
    void loadExport(null, 1);
  }

  function retryExport() {
    if (failedPage) void loadExport(failedPage.cursor, failedPage.index);
  }

  function downloadPage() {
    if (!page || page.count === 0) return;
    const name = `shortcircuit-links-v1-page-${pageNumber}.${exportFormat}`;
    downloadFile(page.file, name);
    toast(tr('admExportDownloaded'), { body: name });
  }

  function clearImport() {
    importFile = null;
    fileName = '';
    csvColumns = null;
    csvSelection = {};
    chosenHost = '';
    report = null;
    importError = null;
  }

  async function goStep(next: 0 | 1 | 2 | 3) {
    step = next;
    await tick();
    importHeading?.focus();
  }

  async function showError(message: keyof Messages) {
    importError = message;
    await tick();
    errorBox?.focus();
  }

  function resetImport() {
    if (busy) return;
    if (fileInput) fileInput.value = '';
    clearImport();
    void goStep(0);
  }

  function back() {
    if (busy) return;
    if (step === 2 && csvColumns !== null) {
      report = null;
      importError = null;
      void goStep(1);
    } else resetImport();
  }

  function chooseFile(event: Event & { currentTarget: HTMLInputElement }) {
    void readFile(event.currentTarget.files?.[0]);
  }

  function drop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    if (!busy) void readFile(event.dataTransfer?.files?.[0]);
  }

  async function readFile(file: File | undefined) {
    if (busy) return;
    clearImport();
    if (!file) return;
    fileName = file.name;
    if (file.size > maxFileBytes) return showError('exchangeFileTooLarge');

    const isCsv = file.name.toLowerCase().endsWith('.csv');
    source = isCsv ? 'csv' : 'json';
    reading = true;
    let failure: keyof Messages | null = null;
    try {
      if (isCsv) {
        const csv = new Blob([file], { type: 'text/csv' });
        const header = await app.api<{ columns: string[]; native: boolean }>(path + '.csv/columns', 'POST', csv);
        if (!header.native) {
          csvColumns = header.columns;
          const names = header.columns.map((name) => name.trim().toLowerCase());
          for (const field of csvFields) {
            const index = names.indexOf(field);
            if (index !== -1 && index === names.lastIndexOf(field)) csvSelection[field] = index;
          }
        }
        importFile = csv;
      } else {
        const data: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await file.arrayBuffer()));
        if (!isExchangeFile(data)) failure = 'exchangeInvalidFile';
        else if (new TextEncoder().encode(JSON.stringify({ ...data, preview: false })).byteLength > maxFileBytes) failure = 'exchangeFileTooLarge';
        else importFile = data;
      }
    } catch (error) {
      if (!isCsv) failure = 'exchangeInvalidJson';
      else if (error instanceof ApiError && error.code === 'invalid_exchange') failure = 'exchangeInvalidCsv';
      else failure = problemMessage(error);
    } finally {
      reading = false;
    }
    if (failure) return showError(failure);
    if (!importFile) return;
    if (csvColumns !== null) await goStep(1);
    else await submitImport(true);
  }

  function changeColumn(field: CsvField, value: string) {
    if (value === '') delete csvSelection[field];
    else csvSelection[field] = Number(value);
    report = null;
    importError = null;
  }

  function csvQuery(preview: boolean): string {
    const query = new URLSearchParams({ preview: String(preview) });
    if (csvColumns !== null) {
      for (const field of csvFields) {
        const column = csvSelection[field];
        if (column !== undefined) query.set(field + '_column', String(column));
      }
      if (csvSelection.host === undefined && importHost) query.set('host', importHost);
    }
    return query.toString();
  }

  function columnName(value: string, index: number): string {
    const label = Array.from(value.trim().replace(/\s+/gu, ' ')).slice(0, 120).join('');
    return num(index + 1) + '. ' + (label || tr('exchangeCsvUnnamedColumn'));
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
    const hasDescription = Object.hasOwn(item, 'description');
    return Object.keys(item).length === (hasDescription ? 8 : 7)
      && (!hasDescription || typeof item.description === 'string')
      && typeof item.host === 'string' && typeof item.slug === 'string'
      && typeof item.target === 'string' && typeof item.title === 'string'
      && typeof item.enabled === 'boolean' && typeof item.analytics_enabled === 'boolean'
      && Array.isArray(item.tags) && item.tags.every((tag: unknown) => typeof tag === 'string');
  }

  async function submitImport(preview: boolean) {
    if (!importFile || busy || !mappingReady || (!preview && !canApply)) return;
    submitting = preview ? 'preview' : 'apply';
    importError = null;
    report = null;
    let failure: keyof Messages | null = null;
    try {
      report = importFile instanceof Blob
        ? await app.api<ImportReport>(path + '.csv?' + csvQuery(preview), 'POST', importFile)
        : await app.api<ImportReport>(path, 'POST', { version: importFile.version, preview, items: importFile.items });
    } catch (error) {
      if (!preview && error instanceof ApiError && error.status === 409) failure = 'exchangeApplyConflict';
      else if (csvColumns !== null && error instanceof ApiError && error.code === 'invalid_exchange') failure = 'exchangeCsvInvalidRows';
      else failure = problemMessage(error);
    } finally {
      submitting = null;
    }
    if (failure) return showError(failure);
    if (report?.applied) {
      toast(tr('exchangeApplied'), { body: tr('admImportSummary', { created: counts.create, unchanged: counts.unchanged }) });
      await goStep(3);
    } else await goStep(2);
  }

  function resultState(status: ItemStatus, applied: boolean): StateView {
    if (status === 'create') return { tone: 'success', icon: 'plus', label: applied ? tr('exchangeCreated') : tr('exchangeWillCreate') };
    if (status === 'unchanged') return { tone: 'muted', icon: 'check', label: tr('exchangeUnchanged') };
    return { tone: 'warning', icon: 'triangle-alert', label: tr('admSlugInUse') };
  }
</script>

<div class="page">
  <PageHeader title={tr('navExchange')} description={tr('admExchangeDescription')} />

  <div class="grid-main-side">
    <section class="panel" aria-labelledby="ex-import-title" aria-busy={busy}>
      <header class="panel-head">
        <div class="panel-title"><h2 id="ex-import-title">{tr('exchangeImport')}</h2><p>{tr('admImportSubtitle')}</p></div>
      </header>
      <ol class="stepper" aria-label={tr('admImportSteps')}>
        {#each STEPS as label, index (label)}
          <li aria-current={index === step ? 'step' : undefined} data-done={index < step || undefined}>
            {#if index < step}<Icon name="check" /><span class="visually-hidden">{tr('admStepCompleted')}:</span>{:else}<span class="num" aria-hidden="true">{index + 1}</span>{/if}{tr(label)}
          </li>
        {/each}
      </ol>
      <div class="panel-body body">
        <h3 class="visually-hidden" tabindex="-1" bind:this={importHeading}>{tr(STEPS[step])}</h3>
        {#if importError}
          <div class="notice" data-tone="error" role="alert" tabindex="-1" bind:this={errorBox}><Icon name="circle-alert" /><div><p>{locale.t[importError]}</p></div></div>
        {/if}

        {#if step === 0}
          <div class="choice-grid" role="radiogroup" aria-label={tr('admImportSource')}>
            <label class="choice"><input class="radio" type="radio" name="ex-source" bind:group={source} value="csv" disabled={busy} /><span><b>{tr('admSourceCsv')}</b><small>{tr('admSourceCsvHint')}</small></span></label>
            <label class="choice"><input class="radio" type="radio" name="ex-source" bind:group={source} value="json" disabled={busy} /><span><b>{tr('admSourceJson')}</b><small>{tr('admSourceJsonHint')}</small></span></label>
            <div class="choice future" aria-disabled="true"><Icon name="arrow-left-right" /><span><b>{tr('admSourceShlink')}</b><small>{tr('admPlanned')}</small></span></div>
          </div>
          <input
            id="ex-file"
            class="visually-hidden"
            type="file"
            tabindex="-1"
            accept={source === 'csv' ? '.csv,text/csv' : '.json,application/json'}
            bind:this={fileInput}
            onchange={chooseFile}
            disabled={busy}
            aria-describedby="ex-file-hint"
          />
          <button
            type="button"
            class="drop"
            data-dragging={dragging || undefined}
            disabled={busy}
            aria-describedby="ex-file-hint"
            onclick={() => fileInput?.click()}
            ondragover={(e) => { e.preventDefault(); dragging = true; }}
            ondragleave={() => (dragging = false)}
            ondrop={drop}
          >
            {#if busy}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="upload" />{/if}
            <b>{busy ? tr('loading') : tr('admChooseOrDrop')}</b>
            <small id="ex-file-hint">{tr('exchangeFileHint')}</small>
          </button>
        {:else if step === 1 && csvColumns}
          <p class="secondary lead"><b class="mono">{fileName}</b>, {tr('admColumnCount', { count: csvColumns.length })}. {tr('exchangeCsvMappingHint')}</p>
          <fieldset class="fieldset" disabled={busy}>
            <legend class="visually-hidden">{tr('exchangeCsvMapping')}</legend>
            <div class="map">
              {#each csvFields as field (field)}
                <label class="field-label" for="ex-map-{field}">{tr(fieldLabels[field])}{#if field === 'target' || field === 'slug'}<span class="visually-hidden"> ({tr('admRequired')})</span>{/if}</label>
                <Icon name="arrow-left-right" />
                <select id="ex-map-{field}" class="select" data-size="sm" value={csvSelection[field]?.toString() ?? ''} onchange={(event) => changeColumn(field, event.currentTarget.value)}>
                  <option value="">{field === 'target' || field === 'slug' ? tr('exchangeCsvSelectColumn') : tr('exchangeCsvSkipColumn')}</option>
                  {#each csvColumns as column, index (index)}
                    <option value={String(index)} disabled={selectedColumns.includes(index) && csvSelection[field] !== index}>{columnName(column, index)}</option>
                  {/each}
                </select>
              {/each}
              {#if csvSelection.host === undefined}
                <label class="field-label" for="ex-map-domain">{tr('exchangeCsvImportDomain')}</label>
                <Icon name="globe" />
                <select id="ex-map-domain" class="select" data-size="sm" value={importHost} onchange={(e) => { chosenHost = e.currentTarget.value; report = null; importError = null; }}>
                  <option value="">{tr('exchangeCsvChooseDomain')}</option>
                  {#each app.domains?.hosts ?? [] as host (host)}<option value={host}>{host}</option>{/each}
                </select>
              {/if}
            </div>
          </fieldset>
          <p class="field-hint">{tr('exchangeCsvDefaults')}</p>
        {:else if (step === 2 || step === 3) && report}
          {#if step === 3}
            <div class="done">
              <span class="done-icon"><Icon name="circle-check" /></span>
              <h3>{tr('exchangeApplied')}</h3>
              <p class="muted">{tr('admImportSummary', { created: counts.create, unchanged: counts.unchanged })}</p>
            </div>
          {:else}
            <div class="review-sum">
              <Badge state={{ tone: 'success', icon: 'plus', label: tr('admCountNew', { count: counts.create }) }} />
              {#if counts.conflict}<Badge state={{ tone: 'warning', icon: 'triangle-alert', label: tr('admCountConflict', { count: counts.conflict }) }} />{/if}
              {#if counts.unchanged}<Badge state={{ tone: 'muted', icon: 'check', label: tr('admCountUnchanged', { count: counts.unchanged }) }} />{/if}
            </div>
            {#if counts.conflict}
              <div class="notice" data-tone="warning" role="status"><Icon name="triangle-alert" /><div><p>{tr('exchangePreviewConflict')}</p></div></div>
            {/if}
          {/if}
          <div class="table-wrap report">
            <table class="table" data-density="compact" aria-label={report.applied ? tr('exchangeApplyReport') : tr('exchangePreviewReport')}>
              <thead><tr><th scope="col">{tr('shortLink')}</th><th scope="col">{tr('admResult')}</th></tr></thead>
              <tbody>
                {#each report.items as item, index (index)}
                  <tr>
                    <td class="mono small">{item.host}/<b>{item.slug}</b></td>
                    <td><Badge state={resultState(item.status, report.applied)} /></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
        {#if submitting === 'apply'}<p class="muted" role="status">{tr('exchangeApplying')}</p>{/if}
      </div>
      {#if step > 0}
        <footer class="panel-foot">
          {#if step < 3}
            <button class="btn" type="button" data-variant="ghost" disabled={busy} onclick={back}><Icon name="chevron-left" />{tr('admBack')}</button>
          {/if}
          <span class="spacer"></span>
          {#if step === 1}
            <button class="btn" type="button" data-variant="primary" disabled={busy || !mappingReady} aria-busy={submitting === 'preview'} onclick={() => submitImport(true)}>
              {#if submitting === 'preview'}<span class="spinner" aria-hidden="true"></span>{/if}{tr('exchangePreview')}
            </button>
          {:else if step === 2}
            {#if !report}
              <button class="btn" type="button" disabled={busy} onclick={() => submitImport(true)}>{tr('admPreviewAgain')}</button>
            {/if}
            <button class="btn" type="button" data-variant="primary" disabled={busy || !canApply} aria-busy={submitting === 'apply'} onclick={() => submitImport(false)}>
              {#if submitting === 'apply'}<span class="spinner" aria-hidden="true"></span>{/if}{tr('exchangeApply')}
            </button>
          {:else}
            <button class="btn" type="button" onclick={resetImport}>{tr('admImportAnother')}</button>
          {/if}
        </footer>
      {/if}
    </section>

    <section class="panel" aria-labelledby="ex-export-title" aria-busy={exportLoading}>
      <header class="panel-head"><div class="panel-title"><h2 id="ex-export-title">{tr('exchangeExport')}</h2><p>{tr('admExportSubtitle')}</p></div></header>
      <div class="panel-body form">
        <div class="field">
          <span class="field-label" id="ex-format">{tr('exchangeFormat')}</span>
          <div class="seg" role="radiogroup" aria-labelledby="ex-format" aria-describedby="ex-format-hint">
            <button type="button" role="radio" aria-checked={exportFormat === 'json'} disabled={exportLoading} onclick={() => changeExportFormat('json')}>JSON</button>
            <button type="button" role="radio" aria-checked={exportFormat === 'csv'} disabled={exportLoading} onclick={() => changeExportFormat('csv')}>CSV</button>
          </div>
          <p class="field-hint" id="ex-format-hint">{exportFormat === 'json' ? tr('admExportJsonHint') : tr('admExportCsvHint')} {tr('exchangeExportScope')}</p>
        </div>
        {#if exportError}
          <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[exportError]}</p></div><button class="btn" data-size="sm" onclick={retryExport}>{tr('tryAgain')}</button></div>
        {/if}
        <dl class="facts" aria-live="polite">
          <dt>{tr('exchangePage')}</dt>
          <dd class="num">{exportLoading ? tr('loading') : page ? num(pageNumber) : '—'}</dd>
          {#if page && page.count !== null && !exportLoading}<dt>{tr('exchangePageLinks')}</dt><dd class="num">{num(page.count)}</dd>{/if}
        </dl>
        {#if page?.count === 0}<p class="muted small">{tr('exchangeNoLinks')}</p>{/if}
        <p class="field-hint">{tr('admExportPages')}</p>
      </div>
      <footer class="panel-foot">
        {#if pageNumber > 1}
          <button class="btn" type="button" data-variant="ghost" data-size="sm" disabled={exportLoading} onclick={() => loadExport(null, 1)}>{tr('exchangeFirstPage')}</button>
        {/if}
        <button class="btn" type="button" data-variant="ghost" data-size="sm" disabled={exportLoading || !page || page.next_after_id === null} onclick={() => page && page.next_after_id !== null && loadExport(page.next_after_id, pageNumber + 1)}>
          {tr('exchangeNextPage')}<Icon name="chevron-right" />
        </button>
        <span class="spacer"></span>
        <button class="btn" type="button" data-variant={exportPrimary ? 'primary' : undefined} disabled={exportLoading || !page || page.count === 0} onclick={downloadPage}>
          <Icon name="download" />{exportFormat === 'csv' ? tr('exchangeDownloadCsv') : tr('exchangeDownloadPage')}
        </button>
      </footer>
    </section>
  </div>
</div>

<style>
  .panel-foot > .spacer {
    flex: 1;
  }
  .stepper {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5);
    border-bottom: 1px solid var(--border-subtle);
    overflow-x: auto;
  }
  .stepper li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: var(--space-3);
    color: var(--text-muted);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .stepper li:not(:last-child)::after {
    content: '';
    width: 24px;
    height: 1px;
    margin-left: var(--space-2);
    background: var(--border);
  }
  .stepper .num {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border-control);
    font-size: var(--text-2xs);
  }
  .stepper li[aria-current='step'] {
    color: var(--text);
    font-weight: var(--weight-medium);
  }
  .stepper li[aria-current='step'] .num {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  .stepper li[data-done] :global(svg) {
    width: 16px;
    height: 16px;
    color: var(--success);
  }
  .body {
    display: grid;
    gap: var(--space-4);
  }
  .future {
    opacity: 0.6;
    cursor: not-allowed;
    border-style: dashed;
  }
  .future :global(svg) {
    flex: none;
    width: 16px;
    height: 16px;
    color: var(--text-muted);
    margin-top: 2px;
  }
  .drop {
    display: grid;
    justify-items: center;
    gap: 6px;
    width: 100%;
    padding: var(--space-8) var(--space-4);
    border: 1px dashed var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface-sunken);
    color: var(--text);
    text-align: center;
  }
  .drop:hover:not(:disabled),
  .drop[data-dragging] {
    border-color: var(--accent);
  }
  .drop :global(svg) {
    width: 22px;
    height: 22px;
    color: var(--text-muted);
  }
  .drop small {
    max-width: 52ch;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .lead,
  .small {
    font-size: var(--text-sm);
  }
  .map {
    display: grid;
    grid-template-columns: max-content 16px minmax(0, 260px);
    align-items: center;
    gap: 10px var(--space-3);
  }
  .map :global(svg) {
    width: 14px;
    height: 14px;
    color: var(--text-muted);
  }
  .review-sum {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .report {
    max-height: 420px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }
  .done {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: var(--space-6) 0;
    text-align: center;
  }
  .done-icon :global(svg) {
    width: 32px;
    height: 32px;
    color: var(--success);
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  @media (max-width: 640px) {
    .map {
      grid-template-columns: minmax(0, 1fr);
      gap: 4px;
    }
    .map :global(svg) {
      display: none;
    }
    .map select {
      margin-bottom: var(--space-3);
    }
  }
</style>
