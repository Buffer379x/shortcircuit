<script lang="ts">
  // QR Studio: design and export the server-rendered QR code of any short link. The preview, the
  // downloads, and the copied PNG all come from the server with the same settings; the server
  // verifies that a code with a logo still decodes. Saved designs are QR templates. The server renders
  // at most two codes at a time, so template tiles show the template colors instead of rendered codes.
  import { onDestroy, untrack } from 'svelte';
  import { ApiError } from '../api';
  import type { CatalogLink, LinkSearchPage, QrStyle, QrTemplate } from '../api';
  import type { Messages } from '../i18n';
  import { problemMessage } from '../format';
  import Icon from '../lib/Icon.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import { app, shortUrl } from '../lib/app.svelte';
  import { num } from '../lib/fmt';
  import { tr } from '../lib/locale.svelte';
  import { openCreate } from '../lib/overlays.svelte';
  import { go, route } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tip } from '../lib/tooltip';
  import { fetchLink } from '../features/detail/link';

  const uid = $props.id();
  const contrastErrorId = `${uid}-contrast`;
  const contentTypes = { svg: 'image/svg+xml', png: 'image/png' } as const;
  type Format = keyof typeof contentTypes;
  const correctionLevels = [['L', '7 %'], ['M', '15 %'], ['Q', '25 %'], ['H', '30 %']] as const;
  type Correction = (typeof correctionLevels)[number][0];
  type Settings = {
    size: number;
    foreground: string;
    background: string;
    transparent: boolean;
    correction: Correction;
    quietZone: number;
    frame: QrStyle['frame'];
  };
  // QR colors are data sent to the renderer, not theme colors.
  const defaults: Settings = {
    size: 384,
    foreground: '#000000', // design-lint: allow, QR data value (server default)
    background: '#ffffff', // design-lint: allow, QR data value (server default)
    transparent: false,
    correction: 'M',
    quietZone: 4,
    frame: 'none',
  };
  const swatches: [keyof Messages, string][] = [
    ['detQrBlack', '#000000'], // design-lint: allow, QR data value
    ['detQrPink', '#b52b7c'], // design-lint: allow, QR data value matching --qr-brand
    ['detQrNavy', '#1f3a6e'], // design-lint: allow, QR data value matching --qr-navy
    ['detQrForest', '#1d5b45'], // design-lint: allow, QR data value matching --qr-forest
  ];
  const resolutions = [256, 384, 512, 768, 1024];
  const supportsCopy = typeof ClipboardItem === 'function' && typeof navigator.clipboard?.write === 'function';
  const requests = new AbortController();
  const maxLogoBytes = 256 * 1024;

  // Links for the picker.
  let links = $state.raw<CatalogLink[] | null>(null);
  let linksError = $state<keyof Messages | null>(null);
  let extra = $state.raw<CatalogLink | null>(null);
  const routeId = $derived(Number(route.id) || null);
  const linkId = $derived(routeId ?? links?.[0]?.id ?? null);
  const link = $derived(links?.find((l) => l.id === linkId) ?? (extra?.id === linkId ? extra : null));
  const url = $derived(link ? shortUrl(link) : '');

  // Design and preview.
  let settings = $state.raw<Settings>(defaults);
  let format = $state<Format>('svg');
  let logo = $state.raw<File | null>(null);
  let logoInput = $state<HTMLInputElement>();
  let logoCanvas = $state<HTMLCanvasElement>();
  let logoError = $state<keyof Messages | null>(null);
  let preview = $state<'loading' | 'ready' | 'error'>('loading');
  let previewSource = $state<string | null>(null);
  let previewError = $state<keyof Messages | null>(null);
  let exporting = $state(false);
  let exportError = $state<keyof Messages | null>(null);
  const previewUrl = $derived(linkId ? qrUrl('svg', settings, linkId) : null);
  const validContrast = $derived((luminance(settings.background) + 0.05) / (luminance(settings.foreground) + 0.05) >= 4.5);
  const canExport = $derived(validContrast && preview === 'ready' && !exporting && linkId !== null);
  const style: QrStyle = $derived({
    size: settings.size,
    foreground: settings.foreground.slice(1),
    background: settings.background.slice(1),
    transparent: settings.transparent,
    correction: settings.correction,
    quiet_zone: settings.quietZone,
    frame: settings.frame,
  });

  // Saved designs.
  let templates = $state.raw<QrTemplate[] | null>(null);
  let templatesError = $state<keyof Messages | null>(null);
  let templateBusy = $state(false);
  let selectedId = $state<number | null>(null);
  let templateName = $state('');
  let nameError = $state<keyof Messages | null>(null);
  let removing = $state.raw<QrTemplate | null>(null);
  let removeOpen = $state(false);
  let removeError = $state<keyof Messages | null>(null);
  const selected = $derived(templates?.find((item) => item.id === selectedId) ?? null);
  const isDefault = $derived(JSON.stringify(settings) === JSON.stringify(defaults));

  onDestroy(() => requests.abort());

  $effect(() => {
    untrack(() => {
      void loadLinks();
      void loadTemplates();
    });
  });

  // A link outside the first catalog pages (or archived) is loaded on its own.
  $effect(() => {
    const id = routeId;
    const list = links;
    if (!id || !list || list.some((l) => l.id === id)) return;
    untrack(() => {
      fetchLink(id).then((found) => { if (routeId === id) extra = found; }).catch(() => {});
    });
  });

  // Render the preview shortly after the last change; a newer change cancels a pending logo upload.
  $effect(() => {
    const current = settings;
    const image = logo;
    const id = linkId;
    if (!validContrast || !id) return;
    const pending = new AbortController();
    const timer = setTimeout(() => {
      if (image) void loadLogoPreview(current, image, id, pending.signal);
      else previewSource = qrUrl('svg', current, id);
    }, 120);
    return () => {
      clearTimeout(timer);
      pending.abort();
    };
  });

  async function loadLinks() {
    linksError = null;
    try {
      const items: CatalogLink[] = [];
      let after: LinkSearchPage['next_cursor'] = null;
      for (let page = 0; page < 10; page++) {
        const result: LinkSearchPage = await app.api<LinkSearchPage>('/links/search', 'POST', { view: 'unarchived', sort: 'title_asc', after: after ?? undefined });
        items.push(...result.items);
        after = result.next_cursor;
        if (!after) break;
      }
      links = items;
    } catch (failure) {
      linksError = problemMessage(failure);
    }
  }

  async function loadTemplates() {
    if (templateBusy) return;
    templateBusy = true;
    templatesError = null;
    try {
      templates = (await app.api<{ items: QrTemplate[] }>('/qr/templates')).items;
    } catch (failure) {
      templatesError = problemMessage(failure);
    } finally {
      templateBusy = false;
    }
  }

  function pick(id: number) {
    resetPreview();
    go('qr', id);
  }

  function luminance(color: string): number {
    const [red, green, blue] = [1, 3, 5].map((start) => {
      const channel = Number.parseInt(color.slice(start, start + 2), 16) / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
  }

  function qrPath(kind: Format, current: Settings, id: number, hasLogo = false): string {
    const query = new URLSearchParams({ format: kind, size: String(current.size) });
    if (current.foreground !== defaults.foreground) query.set('foreground', current.foreground.slice(1));
    if (current.background !== defaults.background) query.set('background', current.background.slice(1));
    if (current.transparent) query.set('transparent', 'true');
    if (current.correction !== defaults.correction) query.set('correction', current.correction);
    if (current.quietZone !== defaults.quietZone) query.set('quiet_zone', String(current.quietZone));
    if (current.frame !== defaults.frame) query.set('frame', current.frame);
    return `/links/${id}/qr${hasLogo ? '/logo' : ''}?${query}`;
  }

  function qrUrl(kind: Format, current: Settings, id: number): string {
    return '/api/v1' + qrPath(kind, current, id);
  }

  function fromStyle(value: QrStyle): Settings {
    return {
      size: value.size,
      foreground: '#' + value.foreground,
      background: '#' + value.background,
      transparent: value.transparent,
      correction: value.correction,
      quietZone: value.quiet_zone,
      frame: value.frame,
    };
  }

  function sameStyle(value: QrStyle) {
    const a = fromStyle(value);
    return a.foreground === settings.foreground && a.background === settings.background && a.transparent === settings.transparent
      && a.correction === settings.correction && a.quietZone === settings.quietZone && a.frame === settings.frame && a.size === settings.size;
  }

  function update(change: Partial<Settings>) {
    const next = { ...settings, ...change };
    if (logo) next.correction = 'H';
    if (linkId && qrUrl('svg', next, linkId) === previewUrl) return;
    settings = next;
    resetPreview();
  }

  function resetPreview() {
    preview = 'loading';
    previewSource = null;
    previewError = null;
    exportError = null;
  }

  function chooseLogo(input: HTMLInputElement) {
    const file = input.files?.[0];
    if (!file) return;
    if (file.size > maxLogoBytes || (file.type && file.type !== 'image/png')) {
      removeLogo();
      logoError = file.size > maxLogoBytes ? 'invalidQrLogoBody' : 'invalidQrLogoType';
      return;
    }
    logoError = null;
    logo = file;
    settings = { ...settings, correction: 'H' };
    resetPreview();
  }

  function removeLogo() {
    logo = null;
    if (logoInput) logoInput.value = '';
    logoError = null;
    resetPreview();
  }

  function retryPreview() {
    resetPreview();
    settings = { ...settings };
  }

  function settlePreview(image: Element, state: 'ready' | 'error') {
    if (previewSource === previewUrl && image.getAttribute('src') === previewSource) preview = state;
  }

  async function fetchQr(kind: Format, current: Settings, image: Blob | null, id: number, signal: AbortSignal): Promise<Blob> {
    const response = image
      ? await app.api<Response>(qrPath(kind, current, id, true), 'POST', image.slice(0, image.size, 'image/png'), 'image', signal)
      : await fetch(qrUrl(kind, current, id), { credentials: 'same-origin', signal });
    if (!response.ok) {
      const problem = await response.json().catch(() => null);
      const code = typeof problem?.code === 'string' ? problem.code : '';
      throw new ApiError(response.status, code);
    }
    const data = await response.blob();
    if (data.type.split(';', 1)[0] !== contentTypes[kind]) throw new Error();
    return data;
  }

  async function loadLogoPreview(current: Settings, image: Blob, id: number, signal: AbortSignal) {
    try {
      const bitmap = await createImageBitmap(await fetchQr('png', current, image, id, signal));
      try {
        if (signal.aborted || logo !== image || settings !== current || linkId !== id) return;
        const context = logoCanvas?.getContext('2d');
        if (!context) throw new Error();
        context.clearRect(0, 0, current.size, current.size);
        context.drawImage(bitmap, 0, 0);
        preview = 'ready';
      } finally {
        bitmap.close();
      }
    } catch (error) {
      if (signal.aborted || logo !== image || settings !== current || linkId !== id) return;
      previewError = error instanceof ApiError ? problemMessage(error) : 'qrPreviewUnavailable';
      preview = 'error';
    }
  }

  async function exportQr(kind: Format, copy = false) {
    if (!canExport || !linkId) return;
    const id = linkId;
    exporting = true;
    exportError = null;
    const data = fetchQr(kind, settings, logo, id, requests.signal);
    try {
      if (copy) {
        // Start the clipboard write directly from the click, including while the PNG is fetched.
        await Promise.all([navigator.clipboard.write([new ClipboardItem({ [contentTypes.png]: data })]), data]);
        toast(tr('qrCopied'));
      } else {
        const blob = await data;
        if (requests.signal.aborted) return;
        const objectUrl = URL.createObjectURL(blob);
        const download = document.createElement('a');
        download.href = objectUrl;
        download.download = `shortcircuit-link-${id}.${kind}`;
        download.click();
        setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
      }
    } catch (error) {
      // A clipboard failure can occur before the browser attaches to the PNG response.
      void data.catch(() => {});
      if (!requests.signal.aborted) exportError = copy && !(error instanceof ApiError) ? 'copyFailed' : problemMessage(error);
    } finally {
      exporting = false;
    }
  }

  function applyTemplate(item: QrTemplate | null) {
    selectedId = item?.id ?? null;
    nameError = null;
    update(item ? fromStyle(item) : { ...defaults });
  }

  async function saveTemplate(event: SubmitEvent) {
    event.preventDefault();
    const label = templateName.trim();
    if (templateBusy || !templates) return;
    nameError = null;
    if (!label || [...label].length > 64 || /\p{Cc}/u.test(label)) {
      nameError = 'invalidQrTemplateName';
      document.getElementById(`${uid}-tpl-name`)?.focus();
      return;
    }
    if (!canExport) return;
    templateBusy = true;
    try {
      const item = await app.api<QrTemplate>('/qr/templates', 'POST', { ...style, name: label });
      templates = [item, ...templates];
      selectedId = item.id;
      templateName = '';
      toast(tr('qrTemplateSaved'), { body: item.name });
    } catch (failure) {
      nameError = problemMessage(failure);
      document.getElementById(`${uid}-tpl-name`)?.focus();
    } finally {
      templateBusy = false;
    }
  }

  function beginRemoval() {
    if (templateBusy || !selected) return;
    removing = selected;
    removeError = null;
    removeOpen = true;
  }

  async function removeTemplate() {
    if (templateBusy || !removing || !templates) return;
    const target = removing;
    removeError = null;
    templateBusy = true;
    try {
      await app.api<void>(`/qr/templates/${target.id}`, 'DELETE');
    } catch (failure) {
      if (!(failure instanceof ApiError) || failure.status !== 404) {
        removeError = problemMessage(failure);
        return;
      }
    } finally {
      templateBusy = false;
    }
    templates = templates.filter((item) => item.id !== target.id);
    selectedId = null;
    removeOpen = false;
    toast(tr('qrTemplateDeleted'), { body: target.name });
  }
</script>

<div class="page">
  <PageHeader title={tr('navQr')} description={tr('detQrDescription')} />

  {#if linksError && !links}
    <div class="stack">
      <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(linksError)}</p>
      <div><button type="button" class="btn" onclick={loadLinks}><Icon name="refresh-cw" />{tr('retry')}</button></div>
    </div>
  {:else if links && !links.length && !extra}
    <Empty icon="qr-code" title={tr('detQrNoLinks')}>
      {#snippet actions()}<button type="button" class="btn" onclick={() => openCreate()}><Icon name="plus" />{tr('newLink')}</button>{/snippet}
      {tr('detQrNoLinksHint')}
    </Empty>
  {:else}
    <div class="studio">
      <section class="panel preview-panel" aria-label={tr('detQrPreview')}>
        <header class="panel-head">
          <div class="field link-pick">
            <label class="visually-hidden" for="{uid}-link">{tr('detQrLink')}</label>
            <select id="{uid}-link" class="select" value={linkId ?? ''} disabled={!links || exporting}
              onchange={(e) => pick(Number(e.currentTarget.value))}>
              {#if !links}<option value="">{tr('loading')}</option>{/if}
              {#if extra && !links?.some((l) => l.id === extra?.id)}<option value={extra.id}>{extra.title || extra.slug} – {extra.host}/{extra.slug}</option>{/if}
              {#each links ?? [] as l (l.id)}<option value={l.id}>{l.title || l.slug} – {l.host}/{l.slug}</option>{/each}
            </select>
          </div>
          {#if url}<CopyButton value={url} variant="secondary" size="md" text={tr('detCopyLink')} />{/if}
          {#if supportsCopy}
            <button type="button" class="btn" disabled={!canExport} onclick={() => exportQr('png', true)}><Icon name="image" />{tr('qrCopyPng')}</button>
          {/if}
        </header>
        <div class="stage">
          {#if !validContrast}
            <p id={contrastErrorId} class="notice" data-tone="error" role="alert"><Icon name="triangle-alert" />{tr('invalidQrContrast')}</p>
          {:else}
            <figure class="code" style:background-color={settings.background}>
              {#if preview === 'error'}
                <div class="stack preview-error">
                  <p role="alert">{tr(previewError ?? 'qrPreviewUnavailable')}</p>
                  <div><button type="button" class="btn" onclick={retryPreview}><Icon name="refresh-cw" />{tr('retry')}</button></div>
                </div>
              {:else}
                {#if logo}
                  <div class="qr-bitmap" role="img" aria-label={tr('detQrAlt', { url })} hidden={preview !== 'ready'}>
                    <canvas bind:this={logoCanvas} width={settings.size} height={settings.size} aria-hidden="true"></canvas>
                  </div>
                {:else if previewSource && previewSource === previewUrl}
                  {#key previewUrl}
                    <img src={previewUrl} alt={tr('detQrAlt', { url })} hidden={preview === 'loading'}
                      onload={(e) => settlePreview(e.currentTarget, 'ready')} onerror={(e) => settlePreview(e.currentTarget, 'error')} />
                  {/key}
                {/if}
                {#if preview === 'loading'}<p class="loading" role="status"><Icon name="loader-circle" />{tr('loading')}</p>{/if}
              {/if}
            </figure>
          {/if}
        </div>
        <footer class="stage-foot">
          <span class="mono url">{url}</span>
          <span class="muted">{settings.transparent ? tr('qrTransparentHint') : tr('detQrServerNote')}</span>
        </footer>
      </section>

      <section class="panel controls" aria-labelledby="{uid}-design">
        <header class="panel-head"><div class="panel-title"><h2 id="{uid}-design">{tr('detQrDesign')}</h2></div></header>
        <div class="panel-body form">
          <fieldset class="fieldset" disabled={exporting}>
            <legend>{tr('detQrSaved')}</legend>
            {#if templatesError && !templates}
              <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(templatesError)}</p>
              <div><button type="button" class="btn" data-size="sm" onclick={loadTemplates}>{tr('retry')}</button></div>
            {:else if !templates}
              <p class="muted" role="status">{tr('loading')}</p>
            {:else}
              <div class="templates">
                <button type="button" class="tpl" aria-pressed={selectedId === null && isDefault}
                  onclick={() => applyTemplate(null)}>
                  <span class="tpl-code" aria-hidden="true"><Icon name="qr-code" /></span><span class="tpl-name">{tr('detQrDefault')}</span>
                </button>
                {#each templates as item (item.id)}
                  <button type="button" class="tpl" aria-pressed={selectedId === item.id && sameStyle(item)} onclick={() => applyTemplate(item)}>
                    <span class="tpl-code" aria-hidden="true" style:background-color={'#' + item.background} style:color={'#' + item.foreground}><Icon name="qr-code" /></span>
                    <span class="tpl-name">{item.name}</span>
                  </button>
                {/each}
              </div>
              {#if selected}
                <div><button type="button" class="btn" data-variant="danger-quiet" data-size="sm" disabled={templateBusy} onclick={beginRemoval}>
                  <Icon name="trash-2" />{tr('detQrDeleteNamed', { name: selected.name })}
                </button></div>
              {/if}
              <form class="save-tpl" novalidate onsubmit={saveTemplate}>
                <div class="field">
                  <label class="field-label" for="{uid}-tpl-name">{tr('qrTemplateName')}</label>
                  <div class="tpl-row">
                    <input id="{uid}-tpl-name" class="input" maxlength="64" bind:value={templateName} disabled={templateBusy}
                      aria-invalid={nameError ? 'true' : undefined} aria-describedby={nameError ? `${uid}-tpl-error` : `${uid}-tpl-hint`} />
                    <button type="submit" class="btn" disabled={templateBusy || !canExport || !templateName.trim()}>{tr('qrSaveTemplate')}</button>
                  </div>
                  {#if nameError}<p id="{uid}-tpl-error" class="field-error"><Icon name="circle-alert" />{tr(nameError)}</p>
                  {:else}<p id="{uid}-tpl-hint" class="field-hint">{tr('detQrSavedHint')}</p>{/if}
                </div>
              </form>
            {/if}
          </fieldset>

          <fieldset class="fieldset" disabled={exporting}>
            <legend>{tr('detQrColor')}</legend>
            <div class="swatches" role="radiogroup" aria-label={tr('qrForeground')}>
              {#each swatches as [label, color] (color)}
                <label class="sw" style:--c={color} use:tip={tr(label)}>
                  <input class="visually-hidden" type="radio" name="{uid}-swatch" checked={settings.foreground === color}
                    onchange={() => { selectedId = null; update({ foreground: color }); }} />
                  <span class="visually-hidden">{tr(label)}</span>
                </label>
              {/each}
            </div>
            <div class="colors">
              <div class="field">
                <label class="field-label" for="{uid}-fg">{tr('qrForeground')}</label>
                <input id="{uid}-fg" class="input color" type="color" value={settings.foreground} aria-invalid={!validContrast}
                  aria-describedby={validContrast ? undefined : contrastErrorId} onchange={(e) => update({ foreground: e.currentTarget.value })} />
              </div>
              <div class="field">
                <label class="field-label" for="{uid}-bg">{tr('qrBackground')}</label>
                <input id="{uid}-bg" class="input color" type="color" value={settings.background} aria-invalid={!validContrast}
                  aria-describedby={validContrast ? undefined : contrastErrorId} onchange={(e) => update({ background: e.currentTarget.value })} />
              </div>
            </div>
            <label class="check">
              <input class="switch" type="checkbox" checked={settings.transparent} onchange={(e) => update({ transparent: e.currentTarget.checked })} />
              <span>{tr('qrTransparent')}<small>{tr('detQrTransparentHint')}</small></span>
            </label>
          </fieldset>

          <div class="field">
            <span class="field-label" id="{uid}-ec">{tr('qrCorrection')}</span>
            <div class="seg" role="radiogroup" aria-labelledby="{uid}-ec">
              {#each correctionLevels as [level, share] (level)}
                <label>
                  <input type="radio" name="{uid}-ec" value={level} checked={settings.correction === level} disabled={exporting || logo !== null}
                    onchange={() => update({ correction: level })} />{level}<span class="muted share">{share}</span>
                </label>
              {/each}
            </div>
            <p class="field-hint">{logo ? tr('qrLogoHint') : tr('detQrCorrectionHint')}</p>
          </div>

          <div class="field">
            <label class="field-label" for="{uid}-qz">{tr('detQrQuietZone')} <span class="optional num">{tr('detQrModules', { count: settings.quietZone })}</span></label>
            <input id="{uid}-qz" class="range" type="range" min="4" max="12" step="1" value={settings.quietZone} disabled={exporting}
              onchange={(e) => update({ quietZone: Number(e.currentTarget.value) })} />
          </div>

          <label class="check">
            <input class="switch" type="checkbox" checked={settings.frame === 'outline'} disabled={exporting}
              onchange={(e) => update({ frame: e.currentTarget.checked ? 'outline' : 'none' })} />
            <span>{tr('detQrFrame')}</span>
          </label>

          <div class="field">
            <span class="field-label" id="{uid}-logo-l">{tr('qrLogo')} <span class="optional">{tr('detOptional')}</span></span>
            <div class="logo-row">
              <label class="drop" class:chosen={logo !== null}>
                <input bind:this={logoInput} class="visually-hidden" type="file" accept="image/png,.png" disabled={exporting}
                  aria-labelledby="{uid}-logo-l" aria-describedby="{uid}-logo-d" aria-invalid={logoError ? 'true' : undefined}
                  onchange={(e) => chooseLogo(e.currentTarget)} />
                <Icon name={logo ? 'circle-check' : 'image'} />
                <span class="truncate">{logo ? logo.name : tr('detQrChooseLogo')}</span>
                <small id="{uid}-logo-d">{logo ? tr('detQrLogoSize', { size: num(Math.ceil(logo.size / 1024)) }) : tr('detQrLogoLimits')}</small>
              </label>
              {#if logo}
                <button type="button" class="btn" data-variant="ghost" data-icon-only aria-label={tr('qrRemoveLogo')} use:tip={tr('qrRemoveLogo')}
                  disabled={exporting} onclick={removeLogo}><Icon name="x" /></button>
              {/if}
            </div>
            {#if logoError}<p class="field-error" role="alert"><Icon name="circle-alert" />{tr(logoError)}</p>{/if}
          </div>
        </div>
        <footer class="panel-foot export" aria-busy={exporting}>
          {#if exportError}<p class="notice export-error" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(exportError)}</p>{/if}
          <select class="select size" aria-label={tr('qrResolution')} value={settings.size} disabled={exporting}
            onchange={(e) => update({ size: Number(e.currentTarget.value) })}>
            {#if !resolutions.includes(settings.size)}<option value={settings.size}>{tr('detQrPixels', { size: settings.size })}</option>{/if}
            {#each resolutions as size (size)}<option value={size}>{tr('detQrPixels', { size })}</option>{/each}
          </select>
          <div class="seg" data-size="sm" role="radiogroup" aria-label={tr('detQrFormat')}>
            <label><input type="radio" name="{uid}-format" value="svg" bind:group={format} />SVG</label>
            <label><input type="radio" name="{uid}-format" value="png" bind:group={format} />PNG</label>
          </div>
          <span class="spacer"></span>
          <button type="button" class="btn" data-variant="primary" disabled={!canExport} use:tip={format === 'svg' ? tr('downloadSvg') : tr('downloadPng')}
            onclick={() => exportQr(format)}>
            <Icon name={exporting ? 'loader-circle' : 'download'} />{tr('detQrDownload')}
          </button>
        </footer>
      </section>
    </div>
  {/if}
</div>

<Dialog bind:open={removeOpen} title={tr('qrDeleteTemplateTitle')} busy={templateBusy} width={420} onclose={() => { removing = null; removeError = null; }}>
  {#if removing}
    <div class="stack">
      <p class="template-name">{removing.name}</p>
      {#if removeError}<p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(removeError)}</p>{/if}
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn" disabled={templateBusy} onclick={() => (removeOpen = false)}>{tr('cancel')}</button>
    <button type="button" class="btn" data-variant="danger" disabled={templateBusy} onclick={removeTemplate}>{tr('qrDeleteTemplate')}</button>
  {/snippet}
</Dialog>

<style>
  .studio {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(320px, 400px);
    gap: var(--space-6);
    align-items: start;
  }
  .link-pick {
    flex: 1;
    min-width: 0;
  }
  .stage {
    display: grid;
    place-items: center;
    min-height: 440px;
    padding: var(--space-8);
    background:
      radial-gradient(circle at 1px 1px, var(--border-subtle) 1px, transparent 0) 0 0 / 16px 16px,
      var(--surface-sunken);
  }
  .code {
    display: grid;
    place-items: center;
    width: 300px;
    aspect-ratio: 1;
    margin: 0;
    padding: 20px;
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-popover);
    box-sizing: content-box;
  }
  .code img,
  .qr-bitmap,
  .qr-bitmap canvas {
    display: block;
    grid-area: 1 / 1;
    width: 100%;
    height: auto;
  }
  .code [hidden] {
    display: none;
  }
  .loading {
    grid-area: 1 / 1;
    color: var(--qr-fg);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
  }
  .preview-error {
    grid-area: 1 / 1;
    padding: var(--space-4);
    color: var(--qr-fg);
    text-align: center;
  }
  .stage-foot {
    display: grid;
    gap: 4px;
    padding: var(--space-4) var(--space-5);
    border-top: 1px solid var(--border-subtle);
    font-size: var(--text-xs);
  }
  .url {
    color: var(--text);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  .form {
    display: grid;
    gap: var(--space-4);
  }
  .templates {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2);
  }
  .tpl {
    display: grid;
    justify-items: center;
    gap: 8px;
    min-width: 0;
    padding: 10px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text-secondary);
    font: inherit;
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }
  .tpl:hover {
    border-color: var(--border-control);
  }
  .tpl:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .tpl[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .tpl-code {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    padding: 4px;
    border-radius: var(--radius-xs);
    background: var(--qr-bg);
    color: var(--qr-fg);
    box-sizing: border-box;
  }
  .tpl-code :global(svg) {
    width: 32px;
    height: 32px;
  }
  .tpl-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tpl-row {
    display: flex;
    gap: var(--space-2);
  }
  .tpl-row .input {
    flex: 1;
    min-width: 0;
  }
  .swatches {
    display: flex;
    gap: 8px;
  }
  .sw {
    width: 32px;
    height: 32px;
    border: 3px solid var(--qr-bg);
    border-radius: var(--radius-sm);
    background: var(--c);
    box-shadow: 0 0 0 1px var(--border);
    cursor: pointer;
    box-sizing: border-box;
  }
  .sw:has(:checked) {
    box-shadow: 0 0 0 2px var(--accent);
  }
  .sw:has(:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 3px;
  }
  .colors {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .color {
    padding-block: 4px;
    cursor: pointer;
  }
  .share {
    font-size: var(--text-2xs);
  }
  .range {
    width: 100%;
    accent-color: var(--accent);
  }
  .logo-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .drop {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 2px 10px;
    flex: 1;
    min-width: 0;
    padding: 12px 14px;
    border: 1px dashed var(--border-control);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .drop :global(svg) {
    grid-row: span 2;
    width: 20px;
    height: 20px;
    color: var(--text-muted);
  }
  .drop.chosen :global(svg) {
    color: var(--success);
  }
  .drop small {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .drop:hover,
  .drop:has(:focus-visible) {
    border-color: var(--accent);
  }
  .drop:has(:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .export {
    position: sticky;
    bottom: 0;
    justify-content: flex-start;
    flex-wrap: wrap;
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
    background: var(--surface);
  }
  .export > .spacer {
    flex: 1;
  }
  .export .size {
    width: auto;
  }
  .export-error {
    flex-basis: 100%;
  }
  .controls {
    position: sticky;
    top: calc(var(--topbar-height) + var(--space-4));
  }
  .template-name {
    overflow-wrap: anywhere;
  }
  @media (max-width: 1100px) {
    .studio {
      grid-template-columns: minmax(0, 1fr);
    }
    .controls {
      position: static;
    }
  }
  @media (max-width: 640px) {
    .stage {
      min-height: 0;
      padding: var(--space-5);
    }
    .code {
      width: min(220px, 100%);
      padding: 14px;
    }
    .preview-panel .panel-head {
      flex-wrap: wrap;
    }
  }
</style>
