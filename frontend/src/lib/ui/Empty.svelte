<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../Icon.svelte';
  import type { IconName } from '../icons';
  let { icon, title, children, actions }: { icon: IconName; title: string; children?: Snippet; actions?: Snippet } = $props();
</script>

<div class="empty">
  <div class="empty-icon"><Icon name={icon} /></div>
  <h3>{title}</h3>
  {#if children}<p>{@render children()}</p>{/if}
  {#if actions}<div class="btn-group">{@render actions()}</div>{/if}
</div>
