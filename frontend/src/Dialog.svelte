<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import type { Messages } from './i18n';

  let { t, title, open, busy = false, onclose, children }: {
    t: Messages;
    title: string;
    open: boolean;
    busy?: boolean;
    onclose: () => void;
    children: Snippet;
  } = $props();
  let dialog: HTMLDialogElement;
  const headingId = $props.id();

  $effect(() => {
    if (open && !dialog.open) {
      dialog.showModal();
      // Start with the first field, or with the first footer action of a confirmation.
      const first = dialog.querySelector<HTMLElement>(
        '.dialog-body :is(input:not([type="hidden"]), select, textarea):not([disabled])',
      ) ?? dialog.querySelector<HTMLElement>('.dialog-foot button:not([disabled]), .dialog-body button:not([disabled])');
      first?.focus();
    }
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} aria-labelledby={headingId} oncancel={(event) => { if (busy) event.preventDefault(); }} onclose={() => { if (open) onclose(); }}>
  <header class="dialog-head between">
    <h2 id={headingId}>{title}</h2>
    <button type="button" class="icon-button ghost" aria-label={t.close} disabled={busy} onclick={() => dialog.close()}>
      <Icon name="close" />
    </button>
  </header>
  {@render children()}
</dialog>
