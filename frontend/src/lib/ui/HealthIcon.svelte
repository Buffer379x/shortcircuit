<script lang="ts">
  // Compact health indicator for dense tables: an icon with a tooltip and an accessible label.
  import Icon from '../Icon.svelte';
  import { healthState, type Health } from '../status';
  import { tip } from '../tooltip';
  import { tr } from '../locale.svelte';
  let { health }: { health: Health } = $props();
  const s = $derived(healthState(health));
</script>

<span class="health" data-tone={s.tone} use:tip={s.label} role="img" aria-label={tr('healthLabel', { state: s.label })}>
  <Icon name={s.icon} />
</span>

<style>
  .health {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    color: var(--text-muted);
  }
  .health :global(svg) {
    width: var(--icon-md);
    height: var(--icon-md);
  }
  /* Healthy is the normal case and stays quiet; problems carry the color. */
  [data-tone='success'] { color: var(--text-muted); }
  [data-tone='warning'] { color: var(--warning); }
  [data-tone='error'] { color: var(--error); }
  [data-tone='muted'] { color: var(--text-disabled); }
</style>
