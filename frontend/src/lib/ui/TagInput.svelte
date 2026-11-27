<script lang="ts">
  // Tag picker: chips plus a combobox that suggests existing tags and can create new ones.
  import Icon from '../Icon.svelte';
  import { app, tagColor } from '../app.svelte';
  import { tr } from '../locale.svelte';

  let { value = $bindable([]), id, describedby }: { value?: string[]; id: string; describedby?: string } = $props();
  let query = $state('');
  let open = $state(false);
  let active = $state(0);
  let input: HTMLInputElement;

  const options = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = app.tags.filter((t) => !value.includes(t.name) && t.name.toLowerCase().includes(q)).map((t) => ({ name: t.name, count: t.link_count, create: false }));
    if (q && !app.tags.some((t) => t.name === q) && !value.includes(q)) list.push({ name: q, count: 0, create: true });
    return list.slice(0, 7);
  });

  function add(name: string) {
    const n = name.trim().replace(/,/g, '');
    if (n && !value.includes(n) && value.length < 20) value = [...value, n];
    query = '';
    active = 0;
    input.focus();
  }

  function remove(name: string) {
    value = value.filter((v) => v !== name);
    input.focus();
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') { open = true; active = Math.min(options.length - 1, active + 1); }
    else if (e.key === 'ArrowUp') active = Math.max(0, active - 1);
    else if ((e.key === 'Enter' || e.key === ',') && (query || open) && options[active]) add(options[active].name);
    else if (e.key === 'Backspace' && !query && value.length) value = value.slice(0, -1);
    else if (e.key === 'Escape' && open) { open = false; e.stopPropagation(); }
    else return;
    e.preventDefault();
  }
</script>

<div class="tag-input" class:open>
  {#each value as name (name)}
    <span class="tag" style:--tag-color={tagColor(name)}>
      <i></i>{name}
      <button type="button" aria-label={tr('tagsRemove', { name })} onclick={() => remove(name)}><Icon name="x" /></button>
    </span>
  {/each}
  <input
    bind:this={input}
    {id}
    bind:value={query}
    role="combobox"
    aria-expanded={open && options.length > 0}
    aria-controls="{id}-list"
    aria-autocomplete="list"
    aria-describedby={describedby}
    aria-activedescendant={open && options[active] ? `${id}-o${active}` : undefined}
    placeholder={value.length ? '' : tr('tagsAdd')}
    autocomplete="off"
    onfocus={() => (open = true)}
    onblur={() => setTimeout(() => (open = false), 120)}
    oninput={() => { open = true; active = 0; }}
    onkeydown={key}
  />
  {#if open && options.length}
    <ul class="popover tag-list" id="{id}-list" role="listbox" aria-label={tr('tagsSuggestions')}>
      {#each options as o, i (o.name)}
        <li
          id="{id}-o{i}"
          role="option"
          aria-selected={i === active}
          class="menu-item"
          data-active={i === active || undefined}
          onpointerdown={(e) => { e.preventDefault(); add(o.name); }}
        >
          {#if o.create}
            <Icon name="plus" />{tr('tagsCreate')} <b>{o.name}</b>
          {:else}
            <i class="sw" style:background={tagColor(o.name)}></i>{o.name}<span class="meta">{tr('tagsLinkCount', { count: o.count })}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .tag-input {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-height: var(--control-md);
    padding: 4px 6px;
    border: 1px solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    cursor: text;
  }
  .tag-input:hover {
    border-color: var(--text-muted);
  }
  .tag-input:has(input:focus-visible) {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  input {
    flex: 1;
    min-width: 90px;
    height: 22px;
    padding: 0 4px;
    border: 0;
    background: transparent;
    font-size: var(--text-md);
  }
  input::placeholder {
    color: var(--text-muted);
  }
  .tag-list {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    max-width: none;
  }
  .sw {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-2xs);
  }
  .tag-list b {
    font-weight: var(--weight-semibold);
  }
</style>
