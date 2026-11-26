<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ApiError } from './api';
  import type { Api, Link, QrStyle } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Messages } from './i18n';
  import QrTemplates from './QrTemplates.svelte';

  let { api, link, t, onclose }: { api: Api; link: Link; t: Messages; onclose: () => void } = $props();

  const qrId = $props.id();
  const contrastErrorId = qrId + '-contrast';
  const contentTypes = { svg: 'image/svg+xml', png: 'image/png' } as const;
  type Format = keyof typeof contentTypes;
  const correctionLevels = ['L', 'M', 'Q', 'H'] as const;
  type Settings = {
    size: number;
    foreground: string;
    background: string;
    transparent: boolean;
    correction: typeof correctionLevels[number];
    quietZone: number;
    frame: QrStyle['frame'];
  };
  const defaults: Settings = {
    size: 384,
    foreground: '#000000',
    background: '#ffffff',
    transparent: false,
    correction: 'M',
    quietZone: 4,
    frame: 'none',
  };

  const resolutions = [256, 384, 512, 768, 1024];
  const quietZones = [4, 5, 6, 7, 8, 9, 10, 11, 12];
  const supportsCopy = typeof ClipboardItem === 'function' && typeof navigator.clipboard?.write === 'function';
  const requests = new AbortController();
  const maxLogoBytes = 256 * 1024;
  let settings = $state.raw<Settings>(defaults);
  let logo = $state.raw<Blob | null>(null);
  let logoInput: HTMLInputElement;
  let logoCanvas = $state<HTMLCanvasElement>();
  let logoError = $state<keyof Messages | null>(null);
  let preview = $state<'loading' | 'ready' | 'error'>('loading');
  let previewSource = $state<string | null>(null);
  let previewError = $state<keyof Messages | null>(null);
  let exporting = $state(false);
  let exportError = $state<keyof Messages | null>(null);
  let copied = $state(false);
  const previewUrl = $derived(qrUrl('svg', settings));
  const validContrast = $derived((luminance(settings.background) + 0.05) / (luminance(settings.foreground) + 0.05) >= 4.5);
  const canExport = $derived(validContrast && preview === 'ready' && !exporting);
  const templateStyle: QrStyle = $derived({
    size: settings.size,
    foreground: settings.foreground.slice(1),
    background: settings.background.slice(1),
    transparent: settings.transparent,
    correction: settings.correction,
    quiet_zone: settings.quietZone,
    frame: settings.frame,
  });

  onDestroy(() => requests.abort());

  $effect(() => {
    const current = settings;
    const image = logo;
    if (!validContrast) return;
    const pending = new AbortController();
    const timer = setTimeout(() => {
      if (image) void loadLogoPreview(current, image, pending.signal);
      else previewSource = qrUrl('svg', current);
    }, 120);
    return () => {
      clearTimeout(timer);
      pending.abort();
    };
  });

  function luminance(color: string): number {
    const [red, green, blue] = [1, 3, 5].map((start) => {
      const channel = Number.parseInt(color.slice(start, start + 2), 16) / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
  }

  function qrPath(format: Format, current: Settings, hasLogo = false): string {
    const query = new URLSearchParams({ format, size: String(current.size) });
    if (current.foreground !== defaults.foreground) query.set('foreground', current.foreground.slice(1));
    if (current.background !== defaults.background) query.set('background', current.background.slice(1));
    if (current.transparent) query.set('transparent', 'true');
    if (current.correction !== defaults.correction) query.set('correction', current.correction);
    if (current.quietZone !== defaults.quietZone) query.set('quiet_zone', String(current.quietZone));
    if (current.frame !== defaults.frame) query.set('frame', current.frame);
    return `/links/${link.id}/qr${hasLogo ? '/logo' : ''}?${query}`;
  }

  function qrUrl(format: Format, current: Settings): string {
    return '/api/v1' + qrPath(format, current);
  }

  function update(change: Partial<Settings>) {
    const next = { ...settings, ...change };
    if (qrUrl('svg', next) === previewUrl) return;
    settings = next;
    resetPreview();
  }

  function resetPreview() {
    preview = 'loading';
    previewSource = null;
    previewError = null;
    exportError = null;
    copied = false;
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
    logo = file.slice(0, file.size, 'image/png');
    settings = { ...settings, correction: 'H' };
    resetPreview();
  }

  function removeLogo() {
    logo = null;
    logoInput.value = '';
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

  function applyStyle(style: QrStyle) {
    update({
      size: style.size,
      foreground: '#' + style.foreground,
      background: '#' + style.background,
      transparent: style.transparent,
      correction: logo ? 'H' : style.correction,
      quietZone: style.quiet_zone,
      frame: style.frame,
    });
  }

  async function fetchQr(format: Format, current: Settings, image: Blob | null, signal: AbortSignal): Promise<Blob> {
    const response = image
      ? await api<Response>(qrPath(format, current, true), 'POST', image, 'image', signal)
      : await fetch(qrUrl(format, current), { credentials: 'same-origin', signal });
    if (!response.ok) {
      const problem = await response.json().catch(() => null);
      const code = typeof problem?.code === 'string' ? problem.code : '';
      throw new ApiError(response.status, code);
    }
    const data = await response.blob();
    if (data.type.split(';', 1)[0] !== contentTypes[format]) throw new Error();
    return data;
  }

  async function loadLogoPreview(current: Settings, image: Blob, signal: AbortSignal) {
    try {
      const bitmap = await createImageBitmap(await fetchQr('png', current, image, signal));
      try {
        if (signal.aborted || logo !== image || settings !== current) return;
        const context = logoCanvas?.getContext('2d');
        if (!context) throw new Error();
        context.clearRect(0, 0, current.size, current.size);
        context.drawImage(bitmap, 0, 0);
        preview = 'ready';
      } finally {
        bitmap.close();
      }
    } catch (error) {
      if (signal.aborted || logo !== image || settings !== current) return;
      previewError = error instanceof ApiError ? problemMessage(error) : 'qrPreviewUnavailable';
      preview = 'error';
    }
  }

  async function exportQr(format: Format, copy = false) {
    if (!canExport) return;
    exporting = true;
    exportError = null;
    copied = false;
    const data = fetchQr(format, settings, logo, requests.signal);
    try {
      if (copy) {
        // Start the clipboard write directly from the click, including while the PNG is fetched.
        await Promise.all([navigator.clipboard.write([new ClipboardItem({ [contentTypes.png]: data })]), data]);
        copied = true;
      } else {
        const blob = await data;
        if (requests.signal.aborted) return;
        const url = URL.createObjectURL(blob);
        const download = document.createElement('a');
        download.href = url;
        download.download = `shortcircuit-link-${link.id}.${format}`;
        download.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    } catch (error) {
      // A clipboard failure can occur before the browser attaches to the PNG response.
      void data.catch(() => {});
      if (!requests.signal.aborted) exportError = copy && !(error instanceof ApiError) ? 'copyFailed' : problemMessage(error);
    } finally {
      exporting = false;
    }
  }
</script>

<Dialog {t} title={t.qrCode} open={true} {onclose}>
  <div class="dialog-body stack">
    <div class="qr-settings">
      <label class="field">
        <span>{t.qrForeground}</span>
        <input type="color" value={settings.foreground} disabled={exporting} aria-invalid={!validContrast}
          aria-describedby={validContrast ? undefined : contrastErrorId} onchange={(event) => update({ foreground: event.currentTarget.value })} />
      </label>
      <label class="field">
        <span>{t.qrBackground}</span>
        <input type="color" value={settings.background} disabled={exporting} aria-invalid={!validContrast}
          aria-describedby={validContrast ? undefined : contrastErrorId} onchange={(event) => update({ background: event.currentTarget.value })} />
      </label>
      <label class="field">
        <span>{t.qrCorrection}</span>
        <select value={settings.correction} disabled={exporting || logo !== null}
          onchange={(event) => update({ correction: event.currentTarget.value as Settings['correction'] })}>
          {#each correctionLevels as level (level)}<option value={level}>{level}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{t.qrQuietZone}</span>
        <select value={settings.quietZone} disabled={exporting} onchange={(event) => update({ quietZone: Number(event.currentTarget.value) })}>
          {#each quietZones as modules (modules)}<option value={modules}>{modules}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{t.qrResolution}</span>
        <select value={settings.size} disabled={exporting} onchange={(event) => update({ size: Number(event.currentTarget.value) })}>
          {#if !resolutions.includes(settings.size)}
            <option value={settings.size}>{settings.size} × {settings.size}</option>
          {/if}
          {#each resolutions as resolution (resolution)}<option value={resolution}>{resolution} × {resolution}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{t.qrFrame}</span>
        <select value={settings.frame} disabled={exporting}
          onchange={(event) => update({ frame: event.currentTarget.value as Settings['frame'] })}>
          <option value="none">{t.qrFrameNone}</option>
          <option value="outline">{t.qrFrameOutline}</option>
        </select>
      </label>
    </div>
    <label class="row">
      <input type="checkbox" checked={settings.transparent} disabled={exporting}
        onchange={(event) => update({ transparent: event.currentTarget.checked })} />{t.qrTransparent}
    </label>
    {#if settings.transparent}<p class="muted">{t.qrTransparentHint}</p>{/if}
    <div class="stack compact">
      <div class="row logo-controls">
        <label class="field">
          <span>{t.qrLogo}</span>
          <input type="file" accept="image/png,.png" bind:this={logoInput} disabled={exporting}
            onchange={(event) => chooseLogo(event.currentTarget)} />
        </label>
        {#if logo}
          <button class="button" type="button" disabled={exporting} onclick={removeLogo}>{t.qrRemoveLogo}</button>
        {/if}
      </div>
      {#if logo}<p class="muted">{t.qrLogoHint}</p>{/if}
      {#if logoError}<p class="notice error" role="alert">{t[logoError]}</p>{/if}
    </div>
    <QrTemplates {api} {t} style={templateStyle} disabled={exporting} canSave={canExport} onapply={applyStyle} />
    {#if validContrast}
      <figure class="qr-content stack compact">
        <div class="qr-preview">
          {#if preview === 'error'}
            <div class="stack compact">
              <p role="alert">{t[previewError ?? 'qrPreviewUnavailable']}</p>
              <button class="button" type="button" onclick={retryPreview}>{t.retry}</button>
            </div>
          {:else}
            {#if settings.transparent && preview === 'ready'}
              <svg viewBox="0 0 1 1" aria-hidden="true"><rect width="1" height="1" fill={settings.background} /></svg>
            {/if}
            {#if logo}
              <div class="qr-bitmap" role="img" aria-label={t.qrCode} hidden={preview !== 'ready'}>
                <canvas bind:this={logoCanvas} width={settings.size} height={settings.size} aria-hidden="true"></canvas>
              </div>
            {:else if previewSource === previewUrl}
              {#key previewUrl}
                <img src={previewUrl} alt={t.qrCode} hidden={preview === 'loading'}
                  onload={(event) => settlePreview(event.currentTarget, 'ready')} onerror={(event) => settlePreview(event.currentTarget, 'error')} />
              {/key}
            {/if}
            {#if preview === 'loading'}<p role="status">{t.loading}</p>{/if}
          {/if}
        </div>
        <figcaption class="mono">{link.host}/{link.slug}</figcaption>
      </figure>
    {:else}
      <p id={contrastErrorId} class="notice error" role="alert">{t.invalidQrContrast}</p>
    {/if}
    {#if exportError}<p class="notice error" role="alert">{t[exportError]}</p>{/if}
    {#if copied}<p class="notice" role="status">{t.qrCopied}</p>{/if}
  </div>
  <footer class="dialog-foot row" aria-busy={exporting}>
    {#if supportsCopy}
      <button class="button" type="button" disabled={!canExport} onclick={() => exportQr('png', true)}><Icon name="copy" />{t.qrCopyPng}</button>
    {/if}
    <button class="button" type="button" disabled={!canExport} onclick={() => exportQr('svg')}><Icon name="download" />{t.downloadSvg}</button>
    <button class="button primary" type="button" disabled={!canExport} onclick={() => exportQr('png')}><Icon name="download" />{t.downloadPng}</button>
  </footer>
</Dialog>

<style>
  .qr-settings {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  input[type='color'] {
    padding-block: 4px;
  }

  .qr-content {
    margin: 0;
    text-align: center;
    overflow-wrap: anywhere;
  }

  .qr-preview {
    display: grid;
    place-items: center;
    inline-size: min(100%, 256px);
    aspect-ratio: 1;
    margin-inline: auto;
  }

  .qr-preview svg, img, .qr-bitmap {
    grid-area: 1 / 1;
    inline-size: 100%;
    block-size: auto;
  }

  .qr-bitmap canvas {
    display: block;
    inline-size: 100%;
    block-size: auto;
  }

  .logo-controls {
    align-items: end;
    flex-wrap: wrap;
  }

  .logo-controls .field {
    flex: 1 1 13rem;
  }

  @media (max-width: 360px) {
    .qr-settings { grid-template-columns: minmax(0, 1fr); }
  }
</style>
