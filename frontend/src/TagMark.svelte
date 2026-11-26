<script lang="ts">
  let { color }: { color?: string } = $props();
</script>

{#if color}<span aria-hidden="true" style:--tag-color={color}></span>{/if}

<style>
  span {
    display: inline-block;
    flex: none;
    width: .7rem;
    height: .7rem;
    border: 1px solid var(--control);
    border-radius: 50%;
    background-color: var(--tag-color);
  }
</style>
