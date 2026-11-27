<script lang="ts">
  // Modal dialog on the native <dialog> element: focus is trapped by the browser, Escape closes,
  // the body scrolls while the header and footer stay visible.
  import type { Snippet } from 'svelte';
  import Icon from '../Icon.svelte';
  import { tr } from '../locale.svelte';

  let {
    open = $bindable(false),
    title,
    description,
    width = 560,
    onclose,
    children,
    footer,
    busy = false,
  }: {
    open?: boolean;
    title: string;
    description?: string;
    width?: number;
    onclose?: () => void;
    children: Snippet;
    footer?: Snippet;
    busy?: boolean;
  } = $props();

  let el: HTMLDialogElement;
  const id = 'dlg-' + Math.random().toString(36).slice(2, 8);

  $effect(() => {
    if (open && !el.open) {
      el.showModal();
      requestAnimationFrame(() => {
        const first = el.querySelector<HTMLElement>('[autofocus], .dialog-body input:not([type=hidden]), .dialog-body select, .dialog-body textarea, .dialog-foot .btn');
        first?.focus();
      });
    } else if (!open && el.open) el.close();
  });

  function closed() {
    open = false;
    onclose?.();
  }
</script>

<dialog
  bind:this={el}
  class="dialog"
  style:--dialog-w="{width}px"
  aria-labelledby="{id}-t"
  aria-describedby={description ? `${id}-d` : undefined}
  onclose={closed}
  oncancel={(e) => busy && e.preventDefault()}
  onclick={(e) => e.target === el && !busy && (open = false)}
>
  {#if open}
    <header class="dialog-head">
      <div>
        <h2 id="{id}-t">{title}</h2>
        {#if description}<p id="{id}-d">{description}</p>{/if}
      </div>
      <button class="btn" data-variant="ghost" data-icon-only aria-label={tr('close')} onclick={() => (open = false)} disabled={busy}>
        <Icon name="x" />
      </button>
    </header>
    <div class="dialog-body">{@render children()}</div>
    {#if footer}<footer class="dialog-foot">{@render footer()}</footer>{/if}
  {/if}
</dialog>
