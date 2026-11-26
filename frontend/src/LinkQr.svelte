<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ApiError } from './api';
  import type { Link } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Messages } from './i18n';

  let { link, t, onclose }: { link: Link; t: Messages; onclose: () => void } = $props();

  const resolutionLabel = $props.id() + '-resolution';
  const contentTypes = { svg: 'image/svg+xml', png: 'image/png' } as const;
  type Format = keyof typeof contentTypes;

  let preview = $state<'loading' | 'ready' | 'error'>('loading');
  const resolutions = [256, 384, 512, 768, 1024];
  let size = $state(384);
  let downloading = $state(false);
  let downloadError = $state<keyof Messages | null>(null);
  const downloads = new AbortController();

  onDestroy(() => downloads.abort());

  function qrUrl(format: Format): string {
    return `/api/v1/links/${link.id}/qr?format=${format}&size=${size}`;
  }

  async function downloadQr(format: Format) {
    if (downloading) return;
    const filename = `shortcircuit-link-${link.id}.${format}`;
    downloading = true;
    downloadError = null;
    try {
      const response = await fetch(qrUrl(format), { credentials: 'same-origin', signal: downloads.signal });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        const code = typeof problem?.code === 'string' ? problem.code : '';
        throw new ApiError(response.status, code);
      }
      const data = await response.blob();
      if (data.type.split(';', 1)[0] !== contentTypes[format]) throw new Error();
      if (downloads.signal.aborted) return;
      const url = URL.createObjectURL(data);
      const download = document.createElement('a');
      download.href = url;
      download.download = filename;
      download.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (error) {
      if (!downloads.signal.aborted) downloadError = problemMessage(error);
    } finally {
      downloading = false;
    }
  }
</script>

<Dialog {t} title={t.qrCode} open={true} {onclose}>
  <div class="dialog-body stack">
    <label class="field">
      <span id={resolutionLabel}>{t.qrResolution}</span>
      <select value={size} aria-labelledby={resolutionLabel} disabled={downloading} onchange={(event) => {
        size = Number(event.currentTarget.value);
        preview = 'loading';
      }}>
        {#each resolutions as resolution (resolution)}<option value={resolution}>{resolution} × {resolution}</option>{/each}
      </select>
    </label>
    <figure class="qr-content stack compact">
      <div class="qr-preview">
        {#if preview === 'error'}
          <div class="stack compact">
            <p role="alert">{t.qrPreviewUnavailable}</p>
            <button class="button" type="button" onclick={() => { preview = 'loading'; }}>{t.retry}</button>
          </div>
        {:else}
          {#key size}<img src={qrUrl('svg')} alt={t.qrCode} hidden={preview === 'loading'} onload={() => { preview = 'ready'; }} onerror={() => { preview = 'error'; }} />{/key}
          {#if preview === 'loading'}<p role="status">{t.loading}</p>{/if}
        {/if}
      </div>
      <figcaption class="mono">{link.host}/{link.slug}</figcaption>
    </figure>
    {#if downloadError}<p class="notice error" role="alert">{t[downloadError]}</p>{/if}
  </div>
  <footer class="dialog-foot row" aria-busy={downloading}>
    <button class="button" type="button" disabled={downloading} onclick={() => downloadQr('svg')}><Icon name="download" />{t.downloadSvg}</button>
    <button class="button primary" type="button" disabled={downloading} onclick={() => downloadQr('png')}><Icon name="download" />{t.downloadPng}</button>
  </footer>
</Dialog>

<style>
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

  img {
    inline-size: 100%;
    block-size: auto;
  }
</style>
