<script lang="ts">
  // The ShortCircuit logo, drawn inline from the brand geometry (design/polar-signal/brand/tools/build_brand.py).
  // The mark uses the accent token and the wordmark the text token, so both themes need no separate files.
  import { WORDMARK } from './logo-paths';
  let { compact = false, tagline = false }: { compact?: boolean; tagline?: boolean } = $props();
</script>

{#if compact}
  <svg class="logo-mark" viewBox="10 8 44 48" role="img" aria-label="ShortCircuit"><g class="mark">{@html WORDMARK.mark}</g></svg>
{:else}
  <svg class="logo" class:tagline viewBox="0 0 {tagline ? WORDMARK.taglineWidth : WORDMARK.width} {tagline ? 67 : 48}" role="img" aria-label={tagline ? 'ShortCircuit, take the shorter route.' : 'ShortCircuit'}>
    <g class="mark" transform="translate(-10 -8)">{@html WORDMARK.mark}</g>
    <path class="word" transform="translate(60 4.6) scale(0.436)" d={WORDMARK.name} />
    {#if tagline}<path class="tagline-path" transform="translate(60 50) scale(0.17)" d={WORDMARK.tagline} />{/if}
  </svg>
{/if}

<style>
  .logo {
    display: block;
    height: 21px;
    width: auto;
  }
  .logo.tagline {
    height: 40px;
  }
  .logo-mark {
    display: block;
    width: 24px;
    height: 26px;
  }
  .mark {
    fill: var(--accent);
  }
  .word {
    fill: var(--text);
  }
  .tagline-path {
    fill: var(--text-secondary);
  }
</style>
