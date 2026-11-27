<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, createApi } from './api';
  import type { Api, Domains, Session, User } from './api';
  import { problemMessage } from './format';
  import type { Language, Messages } from './i18n';
  import Icon from './lib/Icon.svelte';
  import Menu from './lib/ui/Menu.svelte';
  import Logo from './lib/ui/Logo.svelte';
  import { NAV, pageTitle } from './lib/nav';
  import { route, href } from './lib/router.svelte';
  import { theme, setTheme } from './lib/theme.svelte';
  import { toasts, dismiss } from './lib/toast.svelte';
  import { overlays, openCreate } from './lib/overlays.svelte';
  import { app, loadTags } from './lib/app.svelte';
  import { locale, setLanguage, tr } from './lib/locale.svelte';
  import { tip } from './lib/tooltip';
  import { isTimeZone, provideTimeZone } from './timezone';
  import CommandPalette from './features/CommandPalette.svelte';
  import CreateLink from './features/CreateLink.svelte';
  import EditLink from './features/EditLink.svelte';
  import QrDialog from './features/QrDialog.svelte';
  import Auth from './pages/Auth.svelte';
  import Overview from './pages/Overview.svelte';
  import Links from './pages/Links.svelte';
  import LinkDetail from './pages/LinkDetail.svelte';
  import Analytics from './pages/Analytics.svelte';
  import Health from './pages/Health.svelte';
  import QrStudio from './pages/QrStudio.svelte';
  import DomainsPage from './pages/Domains.svelte';
  import Exchange from './pages/Exchange.svelte';
  import Keys from './pages/Keys.svelte';
  import System from './pages/System.svelte';
  import Settings from './pages/Settings.svelte';

  interface InstanceStatus {
    version: string;
    last_backup_status: 'verified' | 'failed' | null;
    last_verified_backup_at: number | null;
    storage: { status: 'healthy' | 'low' | 'unavailable' };
  }

  let timeZone = $state<string>();
  provideTimeZone(() => timeZone);
  let setup = $state(false);
  let sessionLoading = $state(true);
  let sessionError = $state<keyof Messages | null>(null);
  let authNotice = $state<keyof Messages | null>(null);
  let domainError = $state<keyof Messages | null>(null);
  let signOutError = $state<keyof Messages | null>(null);
  let instance = $state<InstanceStatus | null>(null);
  let collapsed = $state(readFlag('shortcircuit-rail'));

  const request = createApi(() => app.user?.csrf);
  const api: Api = async (path, method, data, responseFormat, signal) => {
    const activeUser = app.user;
    try {
      return await request(path, method, data, responseFormat, signal);
    } catch (error) {
      if (activeUser && activeUser === app.user && error instanceof ApiError && error.status === 401) {
        if (path === '/auth/password') void refreshSession();
        else if (error.code !== 'invalid_credentials') resetSession('sessionExpired');
      }
      throw error;
    }
  };
  app.api = api;

  const detailId = $derived(route.page === 'links' && /^\d+$/.test(route.id) ? Number(route.id) : null);
  const activePage = $derived(route.page === 'tags' ? 'links' : route.page);
  // One pink action per view: the global New link is primary only where creating links is the main task.
  const globalPrimary = $derived(['overview', 'links', 'tags'].includes(route.page) && detailId == null);
  const instanceOk = $derived(
    !instance || (instance.storage.status !== 'low' && instance.last_backup_status !== 'failed' && app.counts.unreachable === 0),
  );

  function readFlag(key: string) {
    try {
      return localStorage.getItem(key) === '1';
    } catch {
      return false;
    }
  }

  onMount(() => {
    try {
      const savedLanguage = localStorage.getItem('shortcircuit-language');
      const savedZone = localStorage.getItem('shortcircuit-timezone');
      if (savedZone && isTimeZone(savedZone)) timeZone = savedZone;
      if (savedLanguage === 'en' || savedLanguage === 'de') setLanguage(savedLanguage as Language);
      else setLanguage(navigator.language.toLowerCase().startsWith('de') ? 'de' : 'en');
    } catch {
      setLanguage('en');
    }
    void refreshSession();
  });

  $effect(() => {
    const title = detailId != null ? tr('navLinks') : pageTitle(route.page);
    document.title = app.user ? `${title} – ShortCircuit` : 'ShortCircuit';
  });

  $effect(() => {
    void route.page;
    void route.id;
    overlays.navOpen = false;
    overlays.create = false;
    overlays.palette = false;
    window.scrollTo(0, 0);
    requestAnimationFrame(() => document.querySelector<HTMLElement>('main h1')?.focus({ preventScroll: true }));
  });

  async function refreshSession() {
    sessionLoading = true;
    sessionError = null;
    try {
      const session = await request<Session>('/auth/session');
      const loadData = !app.user && session.user;
      setup = session.setup_required;
      if (!session.user && app.user) resetSession('sessionExpired');
      else app.user = session.user;
      if (loadData) loadSessionData();
    } catch (error) {
      sessionError = problemMessage(error);
    } finally {
      sessionLoading = false;
    }
  }

  async function loadDomains() {
    domainError = null;
    const activeUser = app.user;
    try {
      const result = await api<Domains>('/domains');
      if (activeUser === app.user) app.domains = result;
    } catch (error) {
      if (activeUser === app.user) domainError = problemMessage(error);
    }
  }

  async function loadCounts() {
    try {
      const overview = await api<{ links: { total: number; archived?: number }; active_domains: number; monitoring: { attention: number } }>('/overview');
      app.counts.links = overview.links.total - (overview.links.archived ?? 0);
      app.counts.domains = overview.active_domains;
      app.counts.unreachable = overview.monitoring.attention;
    } catch {
      /* Counts are optional decoration in the sidebar. */
    }
    try {
      instance = await api<InstanceStatus>('/system');
    } catch {
      instance = null;
    }
  }

  function loadSessionData() {
    void loadDomains();
    void loadTags();
    void loadCounts();
  }

  function resetSession(notice: keyof Messages | null = null) {
    app.user = null;
    app.domains = null;
    app.tags = [];
    instance = null;
    authNotice = notice;
    signOutError = null;
  }

  function signedIn(account: User) {
    app.user = account;
    setup = false;
    authNotice = null;
    loadSessionData();
  }

  async function passwordChanged(updated?: User) {
    if (updated) app.user = updated;
    else await refreshSession();
  }

  async function signOut() {
    signOutError = null;
    try {
      await api<void>('/auth/logout', 'POST');
      resetSession();
      location.hash = '#/overview';
    } catch (error) {
      if (app.user) signOutError = problemMessage(error);
    }
  }

  function toggleRail() {
    collapsed = !collapsed;
    try {
      localStorage.setItem('shortcircuit-rail', collapsed ? '1' : '0');
    } catch {
      /* The choice still applies to this page. */
    }
  }

  function keys(e: KeyboardEvent) {
    if (!app.user) return;
    const target = e.target as HTMLElement;
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.isContentEditable;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      overlays.palette = true;
    } else if (!typing && e.key.toLowerCase() === 'n' && !e.metaKey && !e.ctrlKey && !e.altKey && !document.querySelector('dialog[open], [role="dialog"]')) {
      e.preventDefault();
      openCreate();
    } else if (e.key === 'Escape' && overlays.navOpen) overlays.navOpen = false;
  }
</script>

<svelte:window onkeydown={keys} />

{#if app.user && !sessionError}
  <div class="app" id="shell" data-collapsed={collapsed || undefined} data-nav-open={overlays.navOpen || undefined}>
    <a class="skip-link" href="#main" onclick={(e) => { e.preventDefault(); document.querySelector<HTMLElement>('main h1')?.focus(); }}>{tr('skipToContent')}</a>
    {#if overlays.navOpen}<div class="scrim" aria-hidden="true" onclick={() => (overlays.navOpen = false)}></div>{/if}
    <aside class="sidebar" aria-label={tr('navSidebar')}>
      <div class="brand">
        <a href={href('overview')} aria-label="ShortCircuit">
          <Logo compact={collapsed} />
        </a>
        {#if !collapsed}
          <button class="btn collapse-button" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('navCollapse')} use:tip={tr('navCollapse')} onclick={toggleRail}>
            <Icon name="panel-left" />
          </button>
        {/if}
      </div>
      <nav id="main-navigation" aria-label={tr('navMain')} class="nav">
        {#each NAV as group (group.label ?? 'main')}
          <div class="nav-group">
            {#if group.label}<div class="nav-label">{tr(group.label)}</div>{/if}
            {#each group.items as item (item.page)}
              <a
                class="nav-item"
                href={href(item.page)}
                aria-current={activePage === item.page ? 'page' : undefined}
                aria-label={collapsed ? tr(item.label) : undefined}
                use:tip={collapsed ? tr(item.label) : ''}
              >
                <Icon name={item.icon} />
                <span>{tr(item.label)}</span>
                {#if item.page === 'health' && app.counts.unreachable}
                  <span class="count" data-tone="error" aria-label={tr('navUnreachable', { count: app.counts.unreachable })}>{app.counts.unreachable}</span>
                {:else if item.page === 'links' && app.counts.links != null}
                  <span class="count">{app.counts.links}</span>
                {:else if item.page === 'domains' && app.counts.domains != null}
                  <span class="count">{app.counts.domains}</span>
                {/if}
              </a>
            {/each}
          </div>
        {/each}
      </nav>
      <div class="sidebar-foot">
        {#if instance}
          <a class="instance" href={href('system')}>
            <span class="instance-head"><i class="dot" data-ok={instanceOk || undefined} aria-hidden="true"></i>{instanceOk ? tr('instanceRunning') : tr('instanceAttention')}</span>
            <dl>
              <dt>{tr('instanceVersion')}</dt><dd>{instance.version}</dd>
              <dt>{tr('instanceBackup')}</dt>
              <dd>{instance.last_verified_backup_at ? new Intl.DateTimeFormat(locale.language, { dateStyle: 'short' }).format(instance.last_verified_backup_at * 1000) : tr('instanceNoBackup')}</dd>
            </dl>
          </a>
        {/if}
        <div class="user-row">
          <span class="avatar" aria-hidden="true">{app.user.username.slice(0, 2).toUpperCase()}</span>
          <div><span class="truncate">{app.user.username}</span><small>{tr('administrator')}</small></div>
          <Menu label={tr('accountMenu')} icon="ellipsis" size="sm" align="start">
            {#snippet children(close)}
              <a class="menu-item" role="menuitem" href={href('settings')} onclick={close}><Icon name="user" />{tr('accountSettings')}</a>
              {#if collapsed}<button class="menu-item" role="menuitem" onclick={() => { toggleRail(); close(); }}><Icon name="panel-left" />{tr('navExpand')}</button>{/if}
              <div class="menu-sep"></div>
              <button class="menu-item" role="menuitem" onclick={() => { close(); void signOut(); }}><Icon name="log-out" />{tr('signOut')}</button>
            {/snippet}
          </Menu>
        </div>
      </div>
    </aside>

    <div class="main-col">
      <header class="topbar">
        <button class="btn menu-button" data-variant="ghost" data-icon-only aria-label={tr('navOpen')} aria-expanded={overlays.navOpen} aria-controls="main-navigation" onclick={() => (overlays.navOpen = true)}>
          <Icon name="menu" />
        </button>
        <nav class="crumbs" aria-label={tr('navBreadcrumb')}>
          {#if detailId != null}
            <a href={href('links')}>{tr('navLinks')}</a><Icon name="chevron-right" /><span aria-current="page" class="truncate">{app.crumb}</span>
          {:else if route.page === 'tags'}
            <a href={href('links')}>{tr('navLinks')}</a><Icon name="chevron-right" /><span aria-current="page">{tr('navTags')}</span>
          {/if}
        </nav>
        <div class="spacer"></div>
        <button class="search-trigger" title={tr('searchTrigger')} aria-keyshortcuts="Control+K Meta+K" onclick={() => (overlays.palette = true)}>
          <Icon name="search" /><span>{tr('searchTrigger')}</span><kbd class="kbd" aria-hidden="true">Ctrl K</kbd>
        </button>
        <Menu label={tr('themeLabel')} icon={theme.choice === 'system' ? 'monitor' : theme.choice === 'dark' ? 'moon' : 'sun'} width={180}>
          {#snippet children(close)}
            <div class="menu-label">{tr('themeLabel')}</div>
            {#each [['dark', 'moon', 'themeDark'], ['light', 'sun', 'themeLight'], ['system', 'monitor', 'themeSystem']] as const as [value, icon, label] (value)}
              <button class="menu-item" role="menuitemradio" aria-checked={theme.choice === value} onclick={() => { setTheme(value); close(); }}>
                <Icon name={icon} />{tr(label)}
              </button>
            {/each}
          {/snippet}
        </Menu>
        <button class="btn new-link" data-variant={globalPrimary ? 'primary' : 'secondary'} aria-label={tr('newLink')} aria-keyshortcuts="N" onclick={() => openCreate()} use:tip={tr('newLinkShortcut')}>
          <Icon name="plus" /><span>{tr('newLink')}</span>
        </button>
      </header>

      <main id="main" class="content">
        {#if signOutError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[signOutError]}</p></div></div>{/if}
        {#if domainError}
          <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[domainError]}</p></div><button class="btn" data-size="sm" onclick={loadDomains}>{tr('tryAgain')}</button></div>
        {:else if !app.domains}
          <p class="muted" role="status">{tr('loadingPage')}</p>
        {:else}
          {#key route.page + route.id}
            {#if route.page === 'overview'}<Overview />
            {:else if route.page === 'links' && detailId != null}<LinkDetail id={detailId} />
            {:else if route.page === 'links' || route.page === 'tags'}<Links />
            {:else if route.page === 'analytics'}<Analytics />
            {:else if route.page === 'health'}<Health />
            {:else if route.page === 'qr'}<QrStudio />
            {:else if route.page === 'domains'}<DomainsPage onchange={loadDomains} />
            {:else if route.page === 'exchange'}<Exchange />
            {:else if route.page === 'keys'}<Keys />
            {:else if route.page === 'system'}<System />
            {:else if route.page === 'settings'}<Settings bind:timeZone onPasswordChanged={passwordChanged} />
            {:else}<Overview />
            {/if}
          {/key}
        {/if}
      </main>
    </div>
  </div>

  {#if app.domains}
    <CreateLink />
    <EditLink />
    <QrDialog />
  {/if}
  <CommandPalette />
{:else}
  <main id="main" class="solo">
    {#if sessionLoading}
      <p class="muted" role="status">{tr('loadingPage')}</p>
    {:else if sessionError}
      <div class="solo-card">
        <Logo />
        <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[sessionError]}</p></div></div>
        <button class="btn" onclick={refreshSession}>{tr('tryAgain')}</button>
      </div>
    {:else}
      <Auth {setup} notice={authNotice} {signedIn} {refreshSession} />
    {/if}
  </main>
{/if}

<section class="toasts" aria-label={tr('notifications')}>
  <div role="status" aria-live="polite" class="toast-list">
    {#each toasts.filter((t) => t.tone !== 'error') as t (t.id)}
      <div class="toast" data-tone={t.tone}>
        <Icon name={t.tone === 'info' ? 'info' : 'circle-check'} />
        <div><b>{t.title}</b>{#if t.body}<p class="truncate">{t.body}</p>{/if}</div>
        <button class="btn" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('dismiss')} onclick={() => dismiss(t.id)}><Icon name="x" /></button>
      </div>
    {/each}
  </div>
  <div role="alert" class="toast-list">
    {#each toasts.filter((t) => t.tone === 'error') as t (t.id)}
      <div class="toast" data-tone="error">
        <Icon name="circle-alert" />
        <div><b>{t.title}</b>{#if t.body}<p>{t.body}</p>{/if}</div>
        <button class="btn" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('dismiss')} onclick={() => dismiss(t.id)}><Icon name="x" /></button>
      </div>
    {/each}
  </div>
</section>
