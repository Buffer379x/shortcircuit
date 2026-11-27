<script lang="ts">
  // A button that opens a menu. Arrow keys move between items, Escape returns focus to the
  // trigger, and the panel stays inside the viewport.
  import type { Snippet } from 'svelte';
  import Icon from '../Icon.svelte';
  import type { IconName } from '../icons';
  import { tip } from '../tooltip';

  let {
    label,
    icon,
    text,
    variant = 'ghost',
    size = 'md',
    align = 'end',
    width,
    children,
  }: {
    label: string;
    icon?: IconName;
    text?: string;
    variant?: string;
    size?: 'sm' | 'md';
    align?: 'start' | 'end';
    width?: number;
    children: Snippet<[() => void]>;
  } = $props();

  let open = $state(false);
  let trigger: HTMLButtonElement;
  let panel = $state<HTMLDivElement>();
  let pos = $state({ top: 0, left: 0 });
  const id = 'menu-' + Math.random().toString(36).slice(2, 8);

  function items() {
    return [...(panel?.querySelectorAll<HTMLElement>('[role^="menuitem"]:not([disabled])') ?? [])];
  }

  function place() {
    const r = trigger.getBoundingClientRect();
    const w = panel?.offsetWidth ?? width ?? 220;
    const h = panel?.offsetHeight ?? 200;
    let left = align === 'end' ? r.right - w : r.left;
    left = Math.min(Math.max(8, left), innerWidth - w - 8);
    let top = r.bottom + 6;
    if (top + h > innerHeight - 8) top = Math.max(8, r.top - h - 6);
    pos = { top, left };
  }

  async function toggle() {
    open = !open;
    if (open) {
      await Promise.resolve();
      place();
      requestAnimationFrame(() => {
        place();
        items()[0]?.focus();
      });
    }
  }

  function close(focus = true) {
    open = false;
    if (focus) trigger?.focus();
  }

  function onKey(e: KeyboardEvent) {
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown') list[(i + 1) % list.length]?.focus();
    else if (e.key === 'ArrowUp') list[(i - 1 + list.length) % list.length]?.focus();
    else if (e.key === 'Home') list[0]?.focus();
    else if (e.key === 'End') list[list.length - 1]?.focus();
    else if (e.key === 'Escape') close();
    else if (e.key === 'Tab') close(false);
    else return;
    e.preventDefault();
  }

  function outside(e: PointerEvent) {
    if (open && !panel?.contains(e.target as Node) && !trigger.contains(e.target as Node)) close(false);
  }
</script>

<svelte:window onpointerdown={outside} onresize={() => open && place()} onscroll={() => open && close(false)} />

<button
  bind:this={trigger}
  class="btn"
  data-variant={variant}
  data-size={size}
  data-icon-only={text ? undefined : ''}
  aria-label={text ? undefined : label}
  aria-haspopup="menu"
  aria-expanded={open}
  aria-controls={open ? id : undefined}
  onclick={toggle}
  use:tip={label}
>
  {#if icon}<Icon name={icon} />{/if}
  {#if text}<span>{text}</span><Icon name="chevron-down" />{/if}
</button>

{#if open}
  <div
    bind:this={panel}
    {id}
    class="popover"
    role="menu"
    tabindex="-1"
    aria-label={label}
    style:top="{pos.top}px"
    style:left="{pos.left}px"
    style:min-width={width ? `${width}px` : undefined}
    onkeydown={onKey}
  >
    {@render children(() => close())}
  </div>
{/if}
