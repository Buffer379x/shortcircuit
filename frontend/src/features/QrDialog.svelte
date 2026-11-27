<script lang="ts">
  // Quick QR view for one link: the server renders and verifies the code. Saved designs from
  // QR Studio apply here; designing a new one happens in QR Studio.
  import { untrack } from 'svelte';
  import { ApiError } from '../api';
  import type { CatalogLink, QrTemplate } from '../api';
  import { downloadFile, problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Icon from '../lib/Icon.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import { linkCache } from './LinkForm.svelte';
  import { overlays } from '../lib/overlays.svelte';
  import { app, shortUrl } from '../lib/app.svelte';
  import { href } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tr } from '../lib/locale.svelte';

  const TYPES = { svg: 'image/svg+xml', png: 'image/png' } as const;
  type Format = keyof typeof TYPES;

  let open = $state(false);
  let id = $state<number | null>(null);
  let link = $state<Pick<CatalogLink, 'id' | 'host' | 'slug'> | null>(null);
  let templates = $state<QrTemplate[]>([]);
  let templateError = $state<keyof Messages | null>(null);
  let template = $state('default');
  let format = $state<Format>('svg');
  let preview = $state<'loading' | 'ready' | 'error'>('loading');
  let busy = $state(false);
  let attempt = $state(0);
  let downloadError = $state<keyof Messages | null>(null);
  let requests: AbortController | null = null;

  $effect(() => {
    const next = overlays.qrId;
    if (next == null) return;
    untrack(() => start(next));
  });
  $effect(() => {
    if (!open) untrack(() => {
      overlays.qrId = null;
      requests?.abort();
    });
  });

  function start(next: number) {
    requests?.abort();
    requests = new AbortController();
    id = next;
    link = linkCache.get(next) ?? null;
    template = 'default';
    format = 'svg';
    preview = 'loading';
    downloadError = null;
    open = true;
    const signal = requests.signal;
    if (!link) {
      app.api<CatalogLink>(`/links/${next}`, 'GET', undefined, 'json', signal)
        .then((l) => { if (!signal.aborted) link = l; })
        .catch(() => { /* The code still renders; only the address line is missing. */ });
    }
    templateError = null;
    app.api<{ items: QrTemplate[] }>('/qr/templates', 'GET', undefined, 'json', signal)
      .then((r) => { if (!signal.aborted) templates = r.items; })
      .catch((error) => { if (!signal.aborted) templateError = problemMessage(error); });
  }

  const chosen = $derived(templates.find((t) => String(t.id) === template));
  function qrPath(f: Format, size: number) {
    const q = new URLSearchParams({ format: f, size: String(size) });
    if (chosen) {
      q.set('foreground', chosen.foreground);
      q.set('background', chosen.background);
      if (chosen.transparent) q.set('transparent', 'true');
      q.set('correction', chosen.correction);
      q.set('quiet_zone', String(chosen.quiet_zone));
      q.set('frame', chosen.frame);
    }
    return `/links/${id}/qr?${q}`;
  }
  // The preview always uses SVG at a fixed size; a design's own size applies to downloads.
  const previewSrc = $derived(id == null ? '' : '/api/v1' + qrPath('svg', 512));
  const downloadSize = $derived(format === 'png' ? 1024 : chosen?.size ?? 512);
  const url = $derived(link ? shortUrl(link) : '');

  $effect(() => {
    void previewSrc;
    preview = 'loading';
  });

  async function download() {
    if (id == null || busy) return;
    busy = true;
    downloadError = null;
    try {
      const response = await fetch('/api/v1' + qrPath(format, downloadSize), { credentials: 'same-origin', signal: requests?.signal });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        throw new ApiError(response.status, typeof problem?.code === 'string' ? problem.code : '');
      }
      const blob = await response.blob();
      if (blob.type.split(';', 1)[0] !== TYPES[format]) throw new Error();
      downloadFile(blob, `shortcircuit-link-${id}.${format}`);
      toast(tr('lnkQrDownloaded'), { body: `shortcircuit-link-${id}.${format}` });
    } catch (error) {
      if (!requests?.signal.aborted) downloadError = error instanceof ApiError ? problemMessage(error) : 'qrPreviewUnavailable';
    } finally {
      busy = false;
    }
  }
</script>

<Dialog bind:open title={tr('qrCode')} description={url || undefined} width={460}>
  <div class="qr-wrap" aria-busy={preview === 'loading'}>
    {#if preview === 'error'}
      <div class="qr-state">
        <p role="alert">{tr('qrPreviewUnavailable')}</p>
        <button type="button" class="btn" data-size="sm" onclick={() => { preview = 'loading'; attempt++; }}>{tr('retry')}</button>
      </div>
    {/if}
    {#if previewSrc}
      {#key `${previewSrc}#${attempt}`}
        <img src={previewSrc} alt={tr('lnkQrFor', { url: url || tr('qrCode') })} width="220" height="220" hidden={preview !== 'ready'}
          onload={() => (preview = 'ready')} onerror={() => (preview = 'error')} />
      {/key}
    {/if}
    {#if preview === 'loading'}<p class="muted" role="status">{tr('loading')}</p>{/if}
  </div>
  <div class="row">
    <div class="field">
      <label class="field-label" for="qr-t">{tr('lnkQrDesign')}</label>
      <select id="qr-t" class="select" bind:value={template} disabled={busy} aria-describedby={templateError ? 'qr-t-e' : undefined}>
        <option value="default">{tr('lnkQrDefault')}</option>
        {#each templates as t (t.id)}<option value={String(t.id)}>{t.name}</option>{/each}
      </select>
      {#if templateError}<p class="field-hint" id="qr-t-e">{tr('lnkQrTemplatesFailed')}</p>{/if}
    </div>
    <div class="field">
      <span class="field-label" id="qr-f">{tr('exchangeFormat')}</span>
      <div class="seg" role="radiogroup" aria-labelledby="qr-f">
        <button type="button" role="radio" aria-checked={format === 'svg'} onclick={() => (format = 'svg')}>SVG</button>
        <button type="button" role="radio" aria-checked={format === 'png'} onclick={() => (format = 'png')}>{tr('lnkPng1024')}</button>
      </div>
    </div>
  </div>
  {#if downloadError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{tr(downloadError)}</p></div></div>{/if}
  {#snippet footer()}
    <a class="btn" data-variant="ghost" href={href('qr', id ?? undefined)} onclick={() => (open = false)}><Icon name="palette" />{tr('lnkOpenQrStudio')}</a>
    <span class="spacer"></span>
    {#if url}<CopyButton value={url} size="md" variant="secondary" text={tr('lnkCopyLink')} />{/if}
    <button type="button" class="btn" data-variant="primary" disabled={busy || preview !== 'ready'} aria-busy={busy} onclick={download}>
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="download" /><span>{tr('lnkDownload')}</span>
    </button>
  {/snippet}
</Dialog>

<style>
  .qr-wrap {
    display: grid;
    place-items: center;
    min-height: 268px;
    padding: var(--space-6);
    border-radius: var(--radius-lg);
    background: var(--qr-bg);
    color: var(--qr-fg);
  }
  .qr-wrap img {
    display: block;
    width: 220px;
    height: 220px;
  }
  .qr-wrap img[hidden] {
    display: none;
  }
  .qr-state {
    display: grid;
    gap: var(--space-2);
    justify-items: center;
    text-align: center;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--space-4);
    align-items: end;
  }
  @media (max-width: 420px) {
    .row {
      grid-template-columns: 1fr;
    }
  }
</style>
