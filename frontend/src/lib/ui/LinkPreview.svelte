<script lang="ts">
  // A link's preview thumbnail (see `.thumb` in ui.css). The image comes from the admin origin
  // (`GET /api/v1/links/{id}/preview`), so the interface's `img-src 'self'` policy applies. The
  // link's name is already the row or page heading, so the image is decorative (`alt=""`).
  // Without a preview, or if the image fails to load, it shows a placeholder.
  import type { LinkPreviewSummary } from '../../api';
  import Icon from '../Icon.svelte';

  let { id, preview, size = 'row' }: { id: number; preview: LinkPreviewSummary | null | undefined; size?: 'row' | 'card' } = $props();

  let failed = $state<string | null>(null);
  const src = $derived(preview ? `/api/v1/links/${id}/preview?v=${preview.fetched_at}` : null);
  // Explicit dimensions reserve the frame before the image loads.
  const box = $derived(size === 'card' ? { width: 320, height: 180 } : { width: 48, height: 27 });
</script>

<span class="thumb" data-size={size} data-kind={src && failed !== src ? preview?.kind : 'none'}>
  {#if src && failed !== src}
    <img {src} alt="" loading="lazy" decoding="async" width={box.width} height={box.height} onerror={() => (failed = src)} />
  {:else}
    <Icon name="image" size={size === 'card' ? 20 : 14} />
  {/if}
</span>
