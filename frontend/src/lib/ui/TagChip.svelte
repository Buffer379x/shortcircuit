<script lang="ts">
  // A tag with its color. The tag's description, when it has one, appears as a tooltip.
  import { app, tagColor } from '../app.svelte';
  import { tip } from '../tooltip';
  let { name, onclick }: { name: string; onclick?: () => void } = $props();
  const description = $derived(app.tags.find((t) => t.name === name)?.description ?? '');
</script>

{#if onclick}
  <button type="button" class="tag" style:--tag-color={tagColor(name)} {onclick} use:tip={description}><i></i>{name}</button>
{:else}
  <span class="tag" style:--tag-color={tagColor(name)} use:tip={description}><i></i>{name}</span>
{/if}
