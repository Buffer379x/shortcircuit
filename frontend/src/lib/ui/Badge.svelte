<script lang="ts">
  import Icon from '../Icon.svelte';
  import type { StateView } from '../status';
  import { tip } from '../tooltip';
  let { state, quiet = false }: { state: StateView; quiet?: boolean } = $props();
</script>

{#if quiet && state.tone === 'success'}
  <span class="state-quiet"><i class="dot" aria-hidden="true"></i>{state.label}</span>
{:else if state.hint}
  <span class="badge" data-tone={state.tone} use:tip={state.hint}><Icon name={state.icon} />{state.label}</span>
{:else}
  <span class="badge" data-tone={state.tone}><Icon name={state.icon} />{state.label}</span>
{/if}

<style>
  .state-quiet {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 22px;
    color: var(--text-secondary);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    white-space: nowrap;
  }
  .state-quiet .dot {
    color: var(--success);
  }
</style>
