<script lang="ts">
  // Placeholder until the page is ported to Polar Signal.
  import type { User } from '../api';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  let { timeZone = $bindable(), onPasswordChanged }: { timeZone?: string; onPasswordChanged: (user?: User) => Promise<void> } = $props();
  void onPasswordChanged;
</script>

<div class="page"><PageHeader title="Settings" description={timeZone} /></div>
