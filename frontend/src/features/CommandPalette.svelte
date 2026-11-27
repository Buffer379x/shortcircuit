<script lang="ts">
  // Ctrl/Cmd+K: find a link by slug, title, destination, or tag, jump to a page, or run an action.
  import type { CatalogLink, LinkSearchPage } from '../api';
  import Icon from '../lib/Icon.svelte';
  import ShortUrl from '../lib/ui/ShortUrl.svelte';
  import { overlays, openCreate } from '../lib/overlays.svelte';
  import { app } from '../lib/app.svelte';
  import { NAV } from '../lib/nav';
  import { go } from '../lib/router.svelte';
  import { setTheme, theme } from '../lib/theme.svelte';
  import { tr } from '../lib/locale.svelte';
  import type { IconName } from '../lib/icons';

  let el: HTMLDialogElement;
  let input = $state<HTMLInputElement>();
  let q = $state('');
  let active = $state(0);
  let found = $state<CatalogLink[]>([]);
  let timer = 0;
  let request: AbortController | undefined;

  type Item = { kind: 'link' | 'page' | 'action'; id: string; label: string; icon: IconName; run: () => void; link?: CatalogLink };
  const isUrl = $derived(/^https?:\/\/\S+$/i.test(q.trim()));

  const items = $derived.by<Item[]>(() => {
    const s = q.trim().toLowerCase();
    const actions: Item[] = isUrl
      ? [{ kind: 'action', id: 'a-new', label: tr('searchShorten', { url: q.trim() }), icon: 'zap', run: () => openCreate(q.trim()) }]
      : [
          { kind: 'action', id: 'a-new', label: tr('newLink'), icon: 'plus', run: () => openCreate() },
          {
            kind: 'action', id: 'a-theme', label: theme.resolved === 'dark' ? tr('searchThemeLight') : tr('searchThemeDark'),
            icon: theme.resolved === 'dark' ? 'sun' : 'moon', run: () => setTheme(theme.resolved === 'dark' ? 'light' : 'dark'),
          },
        ].filter((a) => !s || a.label.toLowerCase().includes(s)) as Item[];
    const links = found.slice(0, 6).map((l) => ({ kind: 'link' as const, id: 'l' + l.id, label: l.title || l.slug, icon: 'link' as IconName, run: () => go('links', l.id), link: l }));
    const pages = NAV.flatMap((g) => g.items)
      .filter((p) => !s || tr(p.label).toLowerCase().includes(s))
      .map((p) => ({ kind: 'page' as const, id: 'p' + p.page, label: tr(p.label), icon: p.icon, run: () => go(p.page) }));
    return [...actions, ...links, ...pages];
  });

  const groups = $derived([
    ['searchActions', items.filter((i) => i.kind === 'action')],
    ['searchLinks', items.filter((i) => i.kind === 'link')],
    ['searchPages', items.filter((i) => i.kind === 'page')],
  ] as const);

  $effect(() => {
    if (overlays.palette && !el.open) {
      q = '';
      active = 0;
      found = [];
      el.showModal();
      requestAnimationFrame(() => input?.focus());
      void search('');
    } else if (!overlays.palette && el.open) el.close();
  });

  async function search(text: string) {
    if (!app.user) return;
    request?.abort();
    request = new AbortController();
    try {
      const page = await app.api<LinkSearchPage>('/links/search', 'POST', { query: text || undefined, view: 'unarchived', sort: 'newest' }, 'json', request.signal);
      found = page.items;
    } catch {
      found = [];
    }
  }

  function changed() {
    active = 0;
    clearTimeout(timer);
    if (!isUrl) timer = window.setTimeout(() => void search(q.trim()), 160);
  }

  function run(i: Item) {
    overlays.palette = false;
    i.run();
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') active = (active + 1) % Math.max(1, items.length);
    else if (e.key === 'ArrowUp') active = (active - 1 + items.length) % Math.max(1, items.length);
    else if (e.key === 'Enter' && items[active]) run(items[active]);
    else return;
    e.preventDefault();
    document.getElementById('cp-' + items[active]?.id)?.scrollIntoView({ block: 'nearest' });
  }
</script>

<dialog bind:this={el} class="dialog palette" aria-label={tr('searchTrigger')} onclose={() => (overlays.palette = false)} onclick={(e) => e.target === el && (overlays.palette = false)}>
  <div class="search pal-search">
    <Icon name="search" />
    <input
      bind:this={input}
      bind:value={q}
      class="input"
      role="combobox"
      aria-expanded="true"
      aria-controls="cp-list"
      aria-activedescendant={items[active] ? 'cp-' + items[active].id : undefined}
      placeholder={tr('searchPlaceholder')}
      autocomplete="off"
      spellcheck="false"
      oninput={changed}
      onkeydown={key}
    />
    <kbd class="kbd" aria-hidden="true">Esc</kbd>
  </div>
  <div class="pal-list" id="cp-list" role="listbox" aria-label={tr('searchResults')}>
    {#each groups as [label, list] (label)}
      {#if list.length}
        <div class="menu-label" role="presentation">{tr(label)}</div>
        {#each list as item (item.id)}
          {@const i = items.indexOf(item)}
          <div id="cp-{item.id}" role="option" aria-selected={i === active} class="menu-item" data-active={i === active || undefined} tabindex="-1" onclick={() => run(item)} onkeydown={() => {}} onpointermove={() => (active = i)}>
            <Icon name={item.icon} />
            {#if item.link}
              <span class="pal-link"><span class="truncate">{item.label}</span><ShortUrl host={item.link.host} slug={item.link.slug} /></span>
            {:else}
              <span class="truncate">{item.label}</span>
            {/if}
          </div>
        {/each}
      {/if}
    {/each}
    {#if !items.length}<p class="empty-row">{tr('searchNoMatch', { query: q })}</p>{/if}
  </div>
</dialog>

<style>
  .palette {
    --dialog-w: 620px;
    margin-top: 12vh;
    max-height: min(560px, 76dvh);
  }
  .pal-search {
    border-bottom: 1px solid var(--border-subtle);
  }
  .pal-search > :global(svg) {
    left: 18px;
  }
  .pal-search .input {
    height: 54px;
    padding-left: 46px;
    border: 0;
    border-radius: 0;
    background: transparent;
    font-size: var(--text-lg);
    box-shadow: none !important;
  }
  .pal-search .kbd {
    right: 16px;
  }
  .pal-list {
    padding: 6px;
    overflow-y: auto;
  }
  .pal-list .menu-item {
    min-height: 40px;
    cursor: pointer;
  }
  .pal-link {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .pal-link :global(.short-url) {
    font-size: var(--text-xs);
    flex: none;
    max-width: 45%;
  }
  .empty-row {
    padding: 24px;
    color: var(--text-muted);
    text-align: center;
    font-size: var(--text-sm);
  }
</style>
