<script lang="ts">
  // Settings > Public pages: edit the missing, unknown-domain, password, and one-time pages per
  // language, preview drafts in a sandboxed frame rendered by the server, and import, export,
  // or apply starter templates. The server validates and sanitizes everything; this view only
  // collects drafts and shows the server's result.
  import { onMount, tick } from 'svelte';
  import { ApiError } from '../../api';
  import type { Messages } from '../../i18n';
  import { downloadFile, problemMessage } from '../../format';
  import Icon from '../../lib/Icon.svelte';
  import Dialog from '../../lib/ui/Dialog.svelte';
  import { app } from '../../lib/app.svelte';
  import { locale, tr } from '../../lib/locale.svelte';
  import { toast } from '../../lib/toast.svelte';

  type Kind = 'missing' | 'unknown_host' | 'password' | 'once';
  type Lang = 'en' | 'de';
  type Mode = 'structured' | 'custom';
  interface Logo { type: string; data: string }
  interface Draft { page: Kind; lang: Lang; mode: Mode; title?: string; message?: string; button?: string; accent?: string; logo?: Logo; html?: string }
  interface Builtin { page: Kind; lang: Lang; title: string; message: string; button?: string }
  interface PagesState { pages: (Draft & { updated_at: number })[]; builtin: Builtin[]; limits: { html_bytes: number; logo_bytes: number } }
  interface Bundle { format: string; version: number; pages: Draft[] }
  interface ImportReport { pages: { page: Kind; lang: Lang; mode: Mode; removed: string[]; preview: string }[]; bundle: Bundle }
  interface Preset { id: string; name: string; bundle: Bundle }

  const KINDS: [Kind, keyof Messages][] = [
    ['missing', 'ppMissing'], ['unknown_host', 'ppUnknownHost'], ['password', 'ppPassword'], ['once', 'ppOnce'],
  ];
  const LANGS: [Lang, keyof Messages][] = [['en', 'ppLangEn'], ['de', 'ppLangDe']];
  const LOGO_TYPES = ['image/png', 'image/jpeg', 'image/webp'];
  const REMOVED: Record<string, keyof Messages> = {
    script: 'ppRmScript', event_handler: 'ppRmEvent', style_attribute: 'ppRmStyle', unsafe_url: 'ppRmUrl', frame: 'ppRmFrame',
    plugin: 'ppRmPlugin', form: 'ppRmForm', meta: 'ppRmMeta', element: 'ppRmElement', attribute: 'ppRmAttribute',
  };
  const PROBLEMS: Record<string, keyof Messages> = {
    page_too_large: 'ppTooLarge', form_required: 'ppFormMissing', form_duplicate: 'ppFormDuplicate', form_not_supported: 'ppFormNotSupported',
    invalid_placeholder: 'ppBadPlaceholder', unsafe_css: 'ppUnsafeCss', html_required: 'ppHtmlRequired', invalid_accent: 'ppBadAccent',
    accent_contrast: 'ppAccentContrast', invalid_logo: 'ppBadLogo', logo_too_large: 'ppBadLogo', text_too_long: 'ppTooLong',
    invalid_text: 'ppBadText', invalid_format: 'ppInvalidFile', too_many_pages: 'ppInvalidFile', unsupported_version: 'ppBadVersion',
    duplicate_page: 'ppDuplicate', button_not_supported: 'ppBadText', invalid_mode_fields: 'ppInvalidFile',
  };
  const message = (error: unknown): keyof Messages => (error instanceof ApiError && PROBLEMS[error.code]) || problemMessage(error);
  const kindLabel = (kind: Kind) => tr(KINDS.find(([key]) => key === kind)?.[1] ?? 'ppMissing');
  const langLabel = (lang: Lang) => tr(lang === 'de' ? 'ppLangDe' : 'ppLangEn');
  const removedText = (removed: string[]) => removed.map((item) => tr(REMOVED[item] ?? 'ppRmElement')).join(', ');

  let data = $state.raw<PagesState | null>(null);
  let presets = $state.raw<Preset[]>([]);
  let loading = $state(true);
  let loadError = $state<keyof Messages | null>(null);

  let kind = $state<Kind>('missing');
  let lang = $state<Lang>(locale.language === 'de' ? 'de' : 'en');
  let draft = $state<Draft>({ page: 'missing', lang: 'en', mode: 'structured' });
  let logoName = $state('');
  let busy = $state(false);
  let fieldError = $state<keyof Messages | null>(null);
  let notice = $state<string | null>(null);

  let previewUrl = $state<string | null>(null);
  let previewRemoved = $state<string[]>([]);
  let previewError = $state<keyof Messages | null>(null);
  let previewing = $state(false);

  const saved = $derived(data?.pages.find((item) => item.page === kind && item.lang === lang) ?? null);
  const builtin = $derived(data?.builtin.find((item) => item.page === kind && item.lang === lang) ?? null);
  const formPage = $derived(kind === 'password' || kind === 'once');
  const logoLimit = $derived(Math.floor((data?.limits.logo_bytes ?? 65536) / 1024));
  const htmlLimit = $derived(data?.limits.html_bytes ?? 65536);

  function cleaned(value: Draft): Draft {
    const text = (field?: string) => (field?.trim() ? field.trim() : undefined);
    if (value.mode === 'custom') return { page: value.page, lang: value.lang, mode: 'custom', html: value.html ?? '' };
    return {
      page: value.page, lang: value.lang, mode: 'structured',
      title: text(value.title), message: text(value.message),
      button: value.page === 'unknown_host' ? undefined : text(value.button),
      accent: text(value.accent), logo: value.logo,
    };
  }
  function fromSaved(): Draft {
    if (!saved) return { page: kind, lang, mode: 'structured' };
    const { updated_at: _, ...stored } = saved;
    return { ...stored };
  }
  const dirty = $derived(JSON.stringify(cleaned(draft)) !== JSON.stringify(cleaned(fromSaved())));
  const stateLabel = $derived(!saved ? tr('ppStateBuiltin') : saved.mode === 'custom' ? tr('ppStateHtml') : tr('ppStateFields'));

  onMount(() => { void load(); });

  async function load() {
    loading = true;
    loadError = null;
    try {
      [data, presets] = await Promise.all([
        app.api<PagesState>('/settings/public-pages'),
        app.api<{ items: Preset[] }>('/settings/public-pages/presets').then((result) => result.items),
      ]);
      select();
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  function select() {
    draft = fromSaved();
    logoName = '';
    fieldError = null;
    notice = null;
    void preview();
  }

  function choose(nextKind: Kind, nextLang: Lang) {
    kind = nextKind;
    lang = nextLang;
    select();
  }

  function setMode(mode: Mode) {
    if (draft.mode === mode) return;
    fieldError = null;
    if (mode === 'custom') {
      const starter = `<main>\n  <h1>${builtin?.title ?? ''}</h1>\n  ${formPage ? '{{form}}' : '<p></p>'}\n</main>\n`;
      draft = { page: kind, lang, mode, html: saved?.mode === 'custom' ? saved.html : starter };
    } else {
      draft = saved?.mode === 'structured' ? fromSaved() : { page: kind, lang, mode };
    }
  }

  async function preview() {
    previewing = true;
    previewError = null;
    try {
      const result = await app.api<{ url: string; removed: string[] }>('/settings/public-pages/preview', 'POST', cleaned(draft));
      previewUrl = result.url;
      previewRemoved = result.removed;
    } catch (error) {
      previewError = message(error);
    } finally {
      previewing = false;
    }
  }

  async function save() {
    if (busy) return;
    busy = true;
    fieldError = null;
    try {
      const result = await app.api<{ page: Draft; removed: string[] }>('/settings/public-pages', 'PATCH', cleaned(draft));
      data = await app.api<PagesState>('/settings/public-pages');
      draft = fromSaved();
      notice = result.removed.length ? tr('ppSavedRemoved', { items: removedText(result.removed) }) : null;
      toast(tr('ppSaved'));
      void preview();
    } catch (error) {
      fieldError = message(error);
      await tick();
      document.getElementById('pp-error')?.focus();
    } finally {
      busy = false;
    }
  }

  // ---------- Logo and HTML files ----------
  async function chooseLogo(input: HTMLInputElement) {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    fieldError = null;
    if (!LOGO_TYPES.includes(file.type) || file.size > (data?.limits.logo_bytes ?? 0)) {
      fieldError = 'ppBadLogo';
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    let binary = '';
    for (const byte of bytes) binary += String.fromCharCode(byte);
    draft.logo = { type: file.type, data: btoa(binary) };
    logoName = file.name;
    void preview();
  }

  function removeLogo() {
    draft.logo = undefined;
    logoName = '';
    void preview();
  }

  async function chooseHtml(input: HTMLInputElement) {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    fieldError = null;
    if (file.size > htmlLimit) {
      fieldError = 'ppTooLarge';
      return;
    }
    try {
      const html = new TextDecoder('utf-8', { fatal: true }).decode(await file.arrayBuffer());
      draft = { page: kind, lang, mode: 'custom', html };
      notice = tr('ppHtmlImported', { name: file.name });
      await preview();
    } catch {
      fieldError = 'ppInvalidHtmlFile';
    }
  }

  // ---------- Reset ----------
  let resetOpen = $state(false);
  let resetting = $state(false);
  async function reset() {
    if (resetting) return;
    resetting = true;
    try {
      data = await app.api<PagesState>('/settings/public-pages/reset', 'POST', { page: kind, lang });
      resetOpen = false;
      select();
      toast(tr('ppResetDone'));
    } catch (error) {
      toast(tr(message(error)), { tone: 'error' });
    } finally {
      resetting = false;
    }
  }

  // ---------- Template files ----------
  let importOpen = $state(false);
  let importSource = $state('');
  let importBundle = $state.raw<Bundle | null>(null);
  let report = $state.raw<ImportReport | null>(null);
  let importError = $state<string | null>(null);
  let importIndex = $state(0);
  let importing = $state(false);
  const shown = $derived(report?.pages[importIndex] ?? null);

  async function exportFile() {
    try {
      const bundle = await app.api<Bundle>('/settings/public-pages/export');
      downloadFile(new Blob([JSON.stringify(bundle, null, 2) + '\n'], { type: 'application/json' }), 'shortcircuit-public-pages.json');
    } catch (error) {
      toast(tr(problemMessage(error)), { tone: 'error' });
    }
  }

  async function checkBundle(bundle: unknown, source: string) {
    importSource = source;
    importBundle = null;
    report = null;
    importError = null;
    importIndex = 0;
    importOpen = true;
    importing = true;
    try {
      report = await app.api<ImportReport>('/settings/public-pages/import/validate', 'POST', bundle);
      importBundle = report.bundle;
    } catch (error) {
      const text = tr(message(error));
      importError = error instanceof ApiError && error.row ? tr('ppEntryError', { row: error.row, message: text }) : text;
    } finally {
      importing = false;
    }
  }

  async function chooseBundle(input: HTMLInputElement) {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) {
      toast(tr('ppFileTooLarge'), { tone: 'error' });
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await file.arrayBuffer()));
    } catch {
      toast(tr('ppInvalidFile'), { tone: 'error' });
      return;
    }
    await checkBundle(parsed, file.name);
  }

  async function applyImport() {
    if (!importBundle || importing) return;
    importing = true;
    try {
      data = await app.api<PagesState>('/settings/public-pages/import', 'POST', importBundle);
      importOpen = false;
      select();
      toast(tr('ppImported'));
    } catch (error) {
      importError = tr(message(error));
    } finally {
      importing = false;
    }
  }
</script>

<section class="panel" aria-labelledby="st-pages" aria-busy={loading || busy}>
  <header class="panel-head"><div class="panel-title"><h2 id="st-pages">{tr('stPublicPages')}</h2><p>{tr('ppHint')}</p></div></header>
  {#if loadError}
    <div class="panel-body">
      <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[loadError]}</p></div><button class="btn" data-size="sm" onclick={load}>{tr('tryAgain')}</button></div>
    </div>
  {:else if loading || !data}
    <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
  {:else}
    <div class="panel-body pp-editor">
      <div class="pp-pick">
        <div class="field">
          <label class="field-label" for="pp-page">{tr('ppPage')}</label>
          <select id="pp-page" class="select" value={kind} disabled={busy} onchange={(event) => choose(event.currentTarget.value as Kind, lang)}>
            {#each KINDS as [key, label] (key)}<option value={key}>{tr(label)}</option>{/each}
          </select>
        </div>
        <div class="field">
          <span class="field-label" id="pp-lang">{tr('ppLanguage')}</span>
          <div class="seg" role="radiogroup" aria-labelledby="pp-lang">
            {#each LANGS as [key, label] (key)}
              <button type="button" role="radio" aria-checked={lang === key} disabled={busy} onclick={() => choose(kind, key)}>{tr(label)}</button>
            {/each}
          </div>
        </div>
      </div>

      <div class="pp-row">
        <p class="secondary small pp-state" role="status"><Icon name={saved ? 'circle-check' : 'circle-dot'} />{stateLabel}{#if dirty} · {tr('admUnsaved')}{/if}</p>
        {#if saved}<button class="btn" type="button" data-variant="danger-quiet" data-size="sm" disabled={busy} onclick={() => (resetOpen = true)}><Icon name="rotate-ccw" />{tr('ppReset')}</button>{/if}
      </div>

      <div class="field">
        <span class="field-label" id="pp-mode">{tr('ppMode')}</span>
        <div class="seg" role="radiogroup" aria-labelledby="pp-mode">
          <button type="button" role="radio" aria-checked={draft.mode === 'structured'} disabled={busy} onclick={() => setMode('structured')}>{tr('ppModeFields')}</button>
          <button type="button" role="radio" aria-checked={draft.mode === 'custom'} disabled={busy} onclick={() => setMode('custom')}>{tr('ppModeHtml')}</button>
        </div>
      </div>

      {#if draft.mode === 'structured'}
        <div class="field">
          <label class="field-label" for="pp-title">{tr('ppTitle')} <span class="optional">{tr('admOptional')}</span></label>
          <input id="pp-title" class="input" maxlength="120" bind:value={draft.title} placeholder={builtin?.title} disabled={busy} />
        </div>
        <div class="field">
          <label class="field-label" for="pp-message">{tr('ppMessage')} <span class="optional">{tr('admOptional')}</span></label>
          <textarea id="pp-message" class="textarea" maxlength="600" rows="3" bind:value={draft.message} placeholder={builtin?.message} disabled={busy}></textarea>
        </div>
        {#if kind !== 'unknown_host'}
          <div class="field">
            <label class="field-label" for="pp-button">{tr('ppButton')} <span class="optional">{tr('admOptional')}</span></label>
            <input id="pp-button" class="input" maxlength="60" bind:value={draft.button} placeholder={builtin?.button} disabled={busy} aria-describedby={kind === 'missing' ? 'pp-button-hint' : undefined} />
            {#if kind === 'missing'}<p class="field-hint" id="pp-button-hint">{tr('ppButtonMissingHint')}</p>{/if}
          </div>
        {/if}
        <div class="field">
          <label class="field-label" for="pp-accent">{tr('ppAccent')} <span class="optional">{tr('admOptional')}</span></label>
          <input id="pp-accent" class="input mono pp-short" maxlength="7" spellcheck="false" autocomplete="off" bind:value={draft.accent} disabled={busy} aria-describedby="pp-accent-hint" />
          <p class="field-hint" id="pp-accent-hint">{tr('ppAccentHint')}</p>
        </div>
        <div class="field">
          <span class="field-label" id="pp-logo">{tr('ppLogo')} <span class="optional">{tr('admOptional')}</span></span>
          <div class="pp-row">
            <label class="btn">
              <input class="visually-hidden" type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} aria-labelledby="pp-logo" aria-describedby="pp-logo-hint" onchange={(event) => chooseLogo(event.currentTarget)} />
              <Icon name="image" />{tr('ppChooseLogo')}
            </label>
            {#if draft.logo}
              <span class="secondary small truncate">{logoName || tr('ppLogoSaved')}</span>
              <button class="btn" type="button" data-variant="ghost" data-size="sm" disabled={busy} onclick={removeLogo}>{tr('ppRemoveLogo')}</button>
            {/if}
          </div>
          <p class="field-hint" id="pp-logo-hint">{tr('ppLogoHint', { size: logoLimit })}</p>
        </div>
      {:else}
        <div class="notice" data-tone="warning"><Icon name="triangle-alert" /><div><p>{tr('ppHtmlRisk')}</p></div></div>
        <div class="field">
          <label class="field-label" for="pp-html">{tr('ppHtml')}</label>
          <textarea id="pp-html" class="textarea mono pp-code" rows="14" spellcheck="false" bind:value={draft.html} disabled={busy} aria-describedby="pp-html-hint"></textarea>
          <p class="field-hint" id="pp-html-hint">
            {tr('ppPlaceholders')} <code>{'{{host}}'}</code> <code>{'{{lang}}'}</code>
            {#if kind === 'missing'}<code>{'{{root_link}}'}</code>{/if}
            {#if formPage}<code>{'{{form}}'}</code> {tr('ppFormRequiredHint')}{/if}
          </p>
        </div>
      {/if}
      <div class="pp-row">
        <label class="btn" data-variant="ghost">
          <input class="visually-hidden" type="file" accept=".html,.htm,text/html" disabled={busy} onchange={(event) => chooseHtml(event.currentTarget)} />
          <Icon name="upload" />{tr('ppImportHtml')}
        </label>
      </div>

      {#if notice}<div class="notice" role="status"><Icon name="info" /><div><p>{notice}</p></div></div>{/if}
      {#if fieldError}<p class="field-error" id="pp-error" tabindex="-1" role="alert"><Icon name="circle-alert" />{locale.t[fieldError]}</p>{/if}

      <div class="field">
        <div class="pp-row">
          <span class="field-label">{tr('ppPreview')}</span>
          <button class="btn" type="button" data-size="sm" disabled={previewing || busy} aria-busy={previewing} onclick={preview}><Icon name="refresh-cw" />{tr('ppUpdatePreview')}</button>
        </div>
        {#if previewError}
          <p class="field-error" role="alert"><Icon name="circle-alert" />{locale.t[previewError]}</p>
        {:else if previewUrl}
          <iframe class="preview-frame" sandbox="" src={previewUrl} title={tr('ppPreviewTitle', { page: kindLabel(kind), lang: langLabel(lang) })}></iframe>
          <p class="field-hint">{previewRemoved.length ? tr('ppRemoved', { items: removedText(previewRemoved) }) : tr('ppPreviewHint')}</p>
        {/if}
      </div>
    </div>
    <footer class="panel-foot">
      <button class="btn" type="button" data-variant="ghost" disabled={busy || !dirty} onclick={select}>{tr('admDiscard')}</button>
      <button class="btn" type="button" data-variant="primary" disabled={busy || !dirty} aria-busy={busy} onclick={save}>
        {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}{tr('ppSave')}
      </button>
    </footer>
  {/if}
</section>

{#if data}
  <section class="panel" aria-labelledby="pp-templates">
    <header class="panel-head"><div class="panel-title"><h2 id="pp-templates">{tr('ppTemplates')}</h2><p>{tr('ppTemplatesHint')}</p></div></header>
    <div class="setting">
      <div><b>{tr('ppExport')}</b><p>{tr('ppExportHint')}</p></div>
      <div><button class="btn" type="button" onclick={exportFile}><Icon name="download" />{tr('ppExport')}</button></div>
    </div>
    <div class="setting">
      <div><b id="pp-import-label">{tr('ppImport')}</b><p>{tr('ppImportHint')}</p></div>
      <div>
        <label class="btn">
          <input class="visually-hidden" type="file" accept=".json,application/json" aria-labelledby="pp-import-label" onchange={(event) => chooseBundle(event.currentTarget)} />
          <Icon name="upload" />{tr('ppChooseFile')}
        </label>
      </div>
    </div>
    <div class="setting">
      <div><b>{tr('ppStarters')}</b><p>{tr('ppStartersHint')}</p></div>
      <div class="pp-row">
        {#each presets as preset (preset.id)}
          <button class="btn" type="button" onclick={() => checkBundle(preset.bundle, preset.name)}>{tr('ppUseStarter', { name: preset.name })}</button>
        {/each}
      </div>
    </div>
  </section>
{/if}

<Dialog bind:open={resetOpen} title={tr('ppResetTitle', { page: kindLabel(kind), lang: langLabel(lang) })} width={460} busy={resetting}>
  <p class="secondary small">{tr('ppResetText')}</p>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={resetting} onclick={() => (resetOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="button" data-variant="danger" disabled={resetting} aria-busy={resetting} onclick={reset}>
      {#if resetting}<span class="spinner" aria-hidden="true"></span>{/if}{tr('ppReset')}
    </button>
  {/snippet}
</Dialog>

<Dialog bind:open={importOpen} title={tr('ppImportTitle')} description={tr('ppImportFrom', { name: importSource })} width={720} busy={importing}>
  {#if importError}
    <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{importError}</p></div></div>
  {:else if !report}
    <p class="muted" role="status">{tr('loadingPage')}</p>
  {:else}
    <ul class="pp-report">
      {#each report.pages as item, index (item.page + item.lang)}
        <li>
          <button class="btn" type="button" data-variant="ghost" data-size="sm" aria-pressed={importIndex === index} onclick={() => (importIndex = index)}>
            {kindLabel(item.page)} · {langLabel(item.lang)}
          </button>
          <span class="secondary small">{item.mode === 'custom' ? tr('ppModeHtml') : tr('ppModeFields')} · {item.removed.length ? tr('ppRemoved', { items: removedText(item.removed) }) : tr('ppNoRemovals')}</span>
        </li>
      {/each}
    </ul>
    {#if shown}
      <iframe class="preview-frame" sandbox="" src={shown.preview} title={tr('ppPreviewTitle', { page: kindLabel(shown.page), lang: langLabel(shown.lang) })}></iframe>
    {/if}
    <p class="field-hint">{tr('ppImportReplaces', { count: report.pages.length })}</p>
  {/if}
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={importing} onclick={() => (importOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="button" data-variant="danger" disabled={importing || !importBundle} aria-busy={importing} onclick={applyImport}>
      {#if importing}<span class="spinner" aria-hidden="true"></span>{/if}{tr('ppApply')}
    </button>
  {/snippet}
</Dialog>

<style>
  .pp-editor {
    display: grid;
    gap: var(--space-5);
  }
  .pp-pick {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--space-4);
    align-items: end;
  }
  .pp-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .pp-state {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
  }
  .pp-short {
    max-width: 160px;
  }
  .pp-code {
    min-height: 240px;
  }
  .pp-report {
    display: grid;
    gap: var(--space-1);
    margin: 0 0 var(--space-4);
    padding: 0;
    list-style: none;
  }
  .pp-report li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  @media (max-width: 640px) {
    .pp-pick {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
