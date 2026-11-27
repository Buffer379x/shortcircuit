<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, description, actions, children }: { title: string; description?: string; actions?: Snippet; children?: Snippet } = $props();
</script>

<header class="page-header">
  <div class="page-heading">
    <h1 tabindex="-1">{title}</h1>
    {#if description}<p>{description}</p>{/if}
    {#if children}{@render children()}{/if}
  </div>
  {#if actions}<div class="btn-group page-actions">{@render actions()}</div>{/if}
</header>

<style>
  .page-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4) var(--space-6);
    flex-wrap: wrap;
  }
  .page-heading {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .page-heading p {
    color: var(--text-muted);
    font-size: var(--text-md);
  }
  .page-actions {
    flex-wrap: nowrap;
  }
  @media (max-width: 640px) {
    .page-actions {
      width: 100%;
      flex-wrap: wrap;
    }
  }
</style>
