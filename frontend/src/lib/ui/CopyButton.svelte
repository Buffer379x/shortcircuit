<script lang="ts">
  // Copies text and confirms in place: the icon turns into a check for two seconds.
  import Icon from '../Icon.svelte';
  import { tip } from '../tooltip';
  import { toast } from '../toast.svelte';
  import { tr } from '../locale.svelte';
  let { value, label = tr('copyShortLink'), size = 'sm', variant = 'ghost', text }:
    { value: string; label?: string; size?: 'sm' | 'md'; variant?: string; text?: string } = $props();
  let done = $state(false);

  async function copy(e: MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      toast(tr('copyFailed'), { tone: 'error', body: value });
      return;
    }
    done = true;
    if (!text) toast(tr('copied'), { body: value });
    setTimeout(() => (done = false), 2000);
  }
</script>

<button
  type="button"
  class="btn copy"
  data-variant={variant}
  data-size={size}
  data-icon-only={text ? undefined : ''}
  data-done={done || undefined}
  aria-label={text ? undefined : label}
  onclick={copy}
  use:tip={done ? tr('copied') : label}
>
  <Icon name={done ? 'check' : 'copy'} />
  {#if text}<span>{done ? tr('copied') : text}</span>{/if}
</button>

<style>
  .copy[data-done] :global(svg) {
    color: var(--success);
  }
</style>
