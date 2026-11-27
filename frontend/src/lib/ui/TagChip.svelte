<script lang="ts">
  import { tagColor } from '../app.svelte';
  let { name, onclick }: { name: string; onclick?: () => void } = $props();
</script>

{#if onclick}
  <button type="button" class="tag" style:--tag-color={tagColor(name)} {onclick}><i></i>{name}</button>
{:else}
  <span class="tag" style:--tag-color={tagColor(name)}><i></i>{name}</span>
{/if}
