<script lang="ts">
  // Side panel for editing an item without leaving the list. The page behind it is inert.
  import type { Snippet } from 'svelte';
  import Icon from '../Icon.svelte';
  import { tr } from '../locale.svelte';

  let {
    open = $bindable(false),
    title,
    description,
    width = 560,
    children,
    footer,
  }: { open?: boolean; title: string; description?: string; width?: number; children: Snippet; footer?: Snippet } = $props();

  let panel = $state<HTMLElement>();
  let returnTo: HTMLElement | null = null;
  const id = 'drw-' + Math.random().toString(36).slice(2, 8);

  $effect(() => {
    const app = document.getElementById('shell');
    if (open) {
      returnTo = document.activeElement as HTMLElement;
      app?.setAttribute('inert', '');
      requestAnimationFrame(() => panel?.querySelector<HTMLElement>('.dialog-body input, .dialog-body select, .dialog-body textarea')?.focus());
    } else {
      app?.removeAttribute('inert');
      returnTo?.focus();
    }
  });
</script>

{#if open}
  <div class="scrim" aria-hidden="true" onclick={() => (open = false)}></div>
  <div
    bind:this={panel}
    class="drawer"
    style:--drawer-w="{width}px"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{id}-t"
    tabindex="-1"
    onkeydown={(e) => e.key === 'Escape' && (open = false)}
  >
    <header class="dialog-head">
      <div>
        <h2 id="{id}-t">{title}</h2>
        {#if description}<p>{description}</p>{/if}
      </div>
      <button class="btn" data-variant="ghost" data-icon-only aria-label={tr('close')} onclick={() => (open = false)}>
        <Icon name="x" />
      </button>
    </header>
    <div class="dialog-body drawer-body">{@render children()}</div>
    {#if footer}<footer class="dialog-foot">{@render footer()}</footer>{/if}
  </div>
{/if}
