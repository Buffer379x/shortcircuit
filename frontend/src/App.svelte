<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Analytics from './Analytics.svelte';
  import Account from './Account.svelte';
  import { ApiError, createApi } from './api';
  import type { Api, Domains, Session, User } from './api';
  import Catalog from './Catalog.svelte';
  import Auth from './Auth.svelte';
  import { problemMessage } from './format';
  import Exchange from './Exchange.svelte';
  import Icon from './Icon.svelte';
  import { messages } from './i18n';
  import type { Language, Messages, Theme } from './i18n';
  import Keys from './Keys.svelte';
  import Links from './Links.svelte';
  import Preferences from './Preferences.svelte';
  import Monitoring from './Monitoring.svelte';
  import Operations from './Operations.svelte';
  import Overview from './Overview.svelte';
  import { isTimeZone, provideTimeZone } from './timezone';
  import Audit from './Audit.svelte';
  import DestinationPolicy from './DestinationPolicy.svelte';

  type Page = 'overview' | 'links' | 'exchange' | 'analytics' | 'domains' | 'tags' | 'apiKeys' | 'account' | 'operations' | 'monitoring' | 'auditLog';
  const pages = [
    { id: 'overview', icon: 'analytics' },
    { id: 'links', icon: 'link' },
    { id: 'analytics', icon: 'analytics' },
    { id: 'monitoring', icon: 'rule' },
    { id: 'domains', icon: 'globe' },
    { id: 'tags', icon: 'tag' },
    { id: 'exchange', icon: 'exchange' },
    { id: 'apiKeys', icon: 'key' },
    { id: 'account', icon: 'account' },
    { id: 'operations', icon: 'archive' },
    { id: 'auditLog', icon: 'history' },
  ] as const;

  let language = $state<Language>('en');
  let theme = $state<Theme>('dark');
  let timeZone = $state<string>();
  provideTimeZone(() => timeZone);
  let systemDark = $state(false);
  const t = $derived(messages[language]);
  let user = $state<User | null>(null);
  let setup = $state(false);
  let page = $state<Page>('overview');
  let createRequested = $state(false);
  let main = $state<HTMLElement>();
  let sessionLoading = $state(true);
  let sessionError = $state<keyof Messages | null>(null);
  let authNotice = $state<keyof Messages | null>(null);
  let domains = $state<Domains | null>(null);
  let domainError = $state<keyof Messages | null>(null);
  let signingOut = $state(false);
  let menuOpen = $state(false);
  let menuButton = $state<HTMLButtonElement>();

  // Each page has a hash route, so reload, Back, and bookmarks keep the location.
  function pageFromHash(): Page | null {
    const id = location.hash.slice(1);
    return pages.some((entry) => entry.id === id) ? id as Page : null;
  }

  function followHash() {
    const destination = pageFromHash();
    if (destination && destination !== page) {
      page = destination;
      menuOpen = false;
      void tick().then(() => main?.focus());
    }
  }
  let signOutError = $state<keyof Messages | null>(null);

  const request = createApi(() => user?.csrf);
  const api: Api = async (path, method, data, responseFormat, signal) => {
    const activeUser = user;
    try {
      return await request(path, method, data, responseFormat, signal);
    } catch (error) {
      if (activeUser && activeUser === user && error instanceof ApiError && error.status === 401) {
        if (path === '/auth/password') void refreshSession();
        else if (error.code !== 'invalid_credentials') resetSession('sessionExpired');
      }
      throw error;
    }
  };

  onMount(() => {
    try {
      const savedLanguage = localStorage.getItem('shortcircuit-language');
      const savedTheme = localStorage.getItem('shortcircuit-theme');
      const savedZone = localStorage.getItem('shortcircuit-timezone');
      if (savedZone && isTimeZone(savedZone)) timeZone = savedZone;
      if (savedLanguage === 'en' || savedLanguage === 'de') language = savedLanguage;
      if (savedTheme === 'dark' || savedTheme === 'light' || savedTheme === 'system') theme = savedTheme;
    } catch {
      // Preferences can still be used for this page when storage is unavailable.
    }
    const query = matchMedia('(prefers-color-scheme: dark)');
    systemDark = query.matches;
    const change = (event: MediaQueryListEvent) => { systemDark = event.matches; };
    query.addEventListener('change', change);
    page = pageFromHash() ?? page;
    window.addEventListener('popstate', followHash);
    window.addEventListener('hashchange', followHash);
    void refreshSession();
    return () => {
      query.removeEventListener('change', change);
      window.removeEventListener('popstate', followHash);
      window.removeEventListener('hashchange', followHash);
    };
  });

  $effect(() => {
    document.documentElement.lang = language;
    document.documentElement.dataset.theme = theme === 'system' ? systemDark ? 'dark' : 'light' : theme;
    try {
      localStorage.setItem('shortcircuit-language', language);
      localStorage.setItem('shortcircuit-theme', theme);
      if (timeZone) localStorage.setItem('shortcircuit-timezone', timeZone);
      else localStorage.removeItem('shortcircuit-timezone');
    } catch {
      // Allow the controls to work when storage is unavailable.
    }
  });

  async function refreshSession() {
    sessionLoading = true;
    sessionError = null;
    try {
      const session = await request<Session>('/auth/session');
      const loadDomainList = !user && session.user;
      setup = session.setup_required;
      if (!session.user && user) resetSession('sessionExpired');
      else user = session.user;
      if (loadDomainList) void loadDomains();
    } catch (error) {
      sessionError = problemMessage(error);
    } finally {
      sessionLoading = false;
    }
  }

  async function loadDomains() {
    domainError = null;
    const activeUser = user;
    try {
      const result = await api<Domains>('/domains');
      if (activeUser === user) domains = result;
    } catch (error) {
      if (activeUser === user) domainError = problemMessage(error);
    }
  }

  function resetSession(notice: keyof Messages | null = null) {
    user = null;
    domains = null;
    page = 'overview';
    createRequested = false;
    authNotice = notice;
    signOutError = null;
  }

  function signedIn(account: User) {
    user = account;
    setup = false;
    authNotice = null;
    void loadDomains();
  }

  async function passwordChanged(updated?: User) {
    if (updated) user = updated;
    else await refreshSession();
  }

  async function signOut() {
    if (signingOut) return;
    signingOut = true;
    signOutError = null;
    try {
      await api<void>('/auth/logout', 'POST');
      resetSession();
    } catch (error) {
      if (user) signOutError = problemMessage(error);
    } finally {
      signingOut = false;
    }
  }

  function createFromOverview() {
    createRequested = true;
    void navigate('links');
  }

  async function navigate(destination: Page) {
    menuOpen = false;
    if (page === destination) return;
    page = destination;
    if (location.hash !== '#' + destination) history.pushState(null, '', '#' + destination);
    await tick();
    main?.focus();
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape' && menuOpen) { menuOpen = false; menuButton?.focus(); } }} />

<svelte:head><title>{user ? `${t[page]} · ShortCircuit` : 'ShortCircuit'}</title></svelte:head>

<a class="skip" href="#main-content">{t.skip}</a>

{#if user && !sessionError}
  <div class="app-shell">
    <aside class="rail">
      <div class="brand"><img src="/favicon.svg" alt="" />ShortCircuit</div>
      <nav id="main-navigation" class:open={menuOpen} aria-label={t.mainNavigation}>
        {#each pages as entry (entry.id)}
          <button class="nav-item" type="button" aria-current={page === entry.id ? 'page' : undefined} onclick={() => navigate(entry.id)}>
            <Icon name={entry.icon} />{t[entry.id]}
          </button>
        {/each}
      </nav>
      <div class="rail-foot">
        <div class="profile row">
          <span class="avatar" aria-hidden="true"><Icon name="account" /></span>
          <strong title={user.username}>{user.username}</strong>
        </div>
      </div>
    </aside>

    <div class="workspace">
      <header class="topbar">
        <div class="crumb" aria-hidden="true"><span>{t.mainNavigation}</span><Icon name="chevron" /><strong>{t[page]}</strong></div>
        <div class="brand mobile-brand"><img src="/favicon.svg" alt="" />ShortCircuit</div>
        <button class="button menu-toggle" type="button" bind:this={menuButton} aria-expanded={menuOpen} aria-controls="main-navigation"
          onclick={() => menuOpen = !menuOpen}><Icon name={menuOpen ? 'close' : 'rule'} />{t.menu}<span class="visually-hidden">: {t[page]}</span></button>
        <div class="top-actions">
          <Preferences {t} bind:language bind:theme />
          <button type="button" class="button signout" disabled={signingOut} onclick={signOut} aria-label={t.signOut} title={t.signOut}>
            <Icon name="exit" /><span>{t.signOut}</span>
          </button>
        </div>
      </header>
      <main id="main-content" class="main" tabindex="-1" bind:this={main}>
        {#if signOutError}<p class="notice error page-notice" role="alert">{t[signOutError]}</p>{/if}
        {#if page === 'overview'}
          <Overview {t} {language} {api} oncreate={createFromOverview} onnavigate={navigate} />
        {:else if page === 'account'}
          <Account {t} {user} {api} bind:timeZone onPasswordChanged={passwordChanged} />
        {:else if page === 'operations'}
          <Operations {language} {api} />
        {:else if page === 'auditLog'}
          <Audit {t} {language} {api} />
        {:else if page === 'monitoring'}
          <Monitoring {t} {language} {api} {domains} />
        {:else if page === 'exchange'}
          <Exchange {t} {api} {language} {domains} />
        {:else if page === 'analytics'}
          <Analytics {t} {api} {language} />
        {:else if page === 'domains' || page === 'tags'}
          {#key page}<Catalog {t} {api} {language} kind={page} onDomainSaved={loadDomains} />{/key}
          {#if page === 'domains'}<DestinationPolicy {t} {api} {language} />{/if}
        {:else if domains}
          {#if page === 'links'}
            <Links {t} {api} {domains} {language} bind:createRequested />
          {:else}
            <Keys {t} {api} {domains} {language} />
          {/if}
        {:else if domainError}
          <div class="panel panel-pad stack"><p class="notice error" role="alert">{t[domainError]}</p><div><button class="button" type="button" onclick={loadDomains}>{t.retry}</button></div></div>
        {:else}
          <p role="status">{t.loading}</p>
        {/if}
      </main>
    </div>
  </div>
{:else}
  <div class="entry-shell">
    <header class="entry-header"><Preferences {t} bind:language bind:theme /></header>
    <main id="main-content" class="entry-main" tabindex="-1">
      {#if sessionLoading}
        <p role="status">{t.loading}</p>
      {:else if sessionError}
        <section class="panel auth-card stack">
          <div class="brand"><img src="/favicon.svg" alt="" />ShortCircuit</div>
          <p class="notice error" role="alert">{t[sessionError]}</p>
          <button class="button" type="button" onclick={refreshSession}>{t.retry}</button>
        </section>
      {:else}
        <Auth {t} {setup} {api} notice={authNotice} {signedIn} {refreshSession} />
      {/if}
    </main>
  </div>
{/if}
