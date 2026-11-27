<script lang="ts">
  import { icons, type IconName } from './icons';
  let { name, size, label }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="var(--icon-stroke)"
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
  focusable="false">{@html icons[name]}</svg>
