<script lang="ts">
  let { host, slug, showHost = true }: { host: string; slug: string; showHost?: boolean } = $props();
</script>

<span class="short-url" title="https://{host}/{slug}">
  {#if showHost}<span class="host">{host}</span>{/if}<span class="slug">/{slug}</span>
</span>
