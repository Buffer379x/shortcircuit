<script lang="ts">
  // Sign-in, password recovery with a CLI token, and the two-step first-run setup.
  import { tick } from 'svelte';
  import { ApiError } from '../api';
  import type { User } from '../api';
  import { labelAllowed, passwordAllowed, problemMessage } from '../format';
  import type { Language, Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import Logo from '../lib/ui/Logo.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import BrandTraces from '../lib/ui/BrandTraces.svelte';
  import { app } from '../lib/app.svelte';
  import { locale, setLanguage, tr } from '../lib/locale.svelte';
  import { theme, setTheme } from '../lib/theme.svelte';
  import { tip } from '../lib/tooltip';

  let { setup, notice, signedIn, refreshSession }: {
    setup: boolean;
    notice: keyof Messages | null;
    signedIn: (user: User) => void;
    refreshSession: () => Promise<void>;
  } = $props();

  type Field = 'token' | 'username' | 'password';
  let recover = $state(false);
  let step = $state<0 | 1>(0);
  let token = $state('');
  let showToken = $state(false);
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let fieldError = $state<{ field: Field; message: keyof Messages } | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let heading = $state<HTMLHeadingElement>();
  let errorBox = $state<HTMLDivElement>();
  const flow = $derived(setup ? 'setup' : recover ? 'recover' : 'login');
  const passwordOk = $derived(passwordAllowed(password));
  const length = $derived([...password].length);
  const title = $derived(
    flow === 'login' ? tr('signIn') : flow === 'recover' ? tr('recover') : step === 0 ? tr('setup') : tr('admCreateAdminTitle'),
  );
  const intro = $derived(
    flow === 'login' ? tr('admSignInIntro') : flow === 'recover' ? tr('admRecoverIntro') : step === 0 ? tr('admSetupIntro') : tr('admCreateAdminIntro'),
  );

  function errorFor(field: Field) {
    return fieldError?.field === field ? fieldError.message : null;
  }

  function described(...ids: (string | false | null)[]) {
    return ids.filter(Boolean).join(' ') || undefined;
  }

  async function focusHeading() {
    await tick();
    heading?.focus();
  }

  async function fail(field: Field, message: keyof Messages) {
    fieldError = { field, message };
    await tick();
    document.getElementById(`auth-${field}`)?.focus();
  }

  async function failForm(message: keyof Messages) {
    formError = message;
    await tick();
    errorBox?.focus();
  }

  function clearErrors() {
    fieldError = null;
    formError = null;
  }

  function switchFlow(next: boolean) {
    recover = next;
    clearErrors();
    token = '';
    password = '';
    showToken = false;
    void focusHeading();
  }

  function goStep(next: 0 | 1) {
    clearErrors();
    step = next;
    void focusHeading();
  }

  function continueSetup(event: SubmitEvent) {
    event.preventDefault();
    clearErrors();
    if (!token.trim()) {
      void fail('token', 'admTokenRequired');
      return;
    }
    goStep(1);
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    clearErrors();
    const name = username.trim();
    if (flow === 'recover' && !token.trim()) return fail('token', 'admTokenRequired');
    if (flow === 'login' && !name) return fail('username', 'admUsernameRequired');
    if (flow === 'setup' && !labelAllowed(name)) return fail('username', 'invalidUsername');
    if (flow === 'login' && !password) return fail('password', 'admPasswordRequired');
    if (flow !== 'login' && !passwordAllowed(password)) return fail('password', 'invalidPassword');

    const payload = flow === 'login' ? { username: name, password }
      : flow === 'setup' ? { token: token.trim(), username: name, password }
        : { token: token.trim(), password };
    busy = true;
    let message: keyof Messages | null = null;
    try {
      signedIn(await app.api<User>(`/auth/${flow}`, 'POST', payload));
      token = '';
      password = '';
    } catch (failure) {
      if (flow === 'setup' && failure instanceof ApiError && failure.code === 'auth_conflict') {
        busy = false;
        await refreshSession();
        return;
      }
      message = problemMessage(failure, flow === 'login' ? 'invalidCredentials' : 'invalidToken');
    } finally {
      busy = false;
    }
    if (!message) return;
    if (message === 'invalidToken' && flow === 'setup') {
      step = 0;
      await fail('token', message);
    } else if (message === 'invalidToken') await fail('token', message);
    else if (message === 'invalidPassword') await fail('password', message);
    else if (message === 'invalidUsername') await fail('username', message);
    else await failForm(message);
  }
</script>

{#snippet error(field: Field)}
  {@const message = errorFor(field)}
  {#if message}<p class="field-error" id="auth-{field}-error"><Icon name="circle-alert" />{locale.t[message]}</p>{/if}
{/snippet}

{#snippet tokenField(label: string)}
  <div class="field">
    <label class="field-label" for="auth-token">{label}</label>
    <div class="input-group">
      <input
        id="auth-token"
        class="input mono"
        type={showToken ? 'text' : 'password'}
        name="token"
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        bind:value={token}
        disabled={busy}
        aria-invalid={errorFor('token') ? 'true' : undefined}
        aria-describedby={described(!!errorFor('token') && 'auth-token-error', 'auth-token-hint')}
      />
      <button
        type="button"
        class="btn"
        data-variant="ghost"
        data-icon-only
        aria-label={showToken ? tr('admHideToken') : tr('admShowToken')}
        aria-pressed={showToken}
        use:tip={showToken ? tr('admHideToken') : tr('admShowToken')}
        onclick={() => (showToken = !showToken)}
      ><Icon name={showToken ? 'eye-off' : 'eye'} /></button>
    </div>
    {@render error('token')}
    <p class="field-hint" id="auth-token-hint">{tr('admConsoleHint')}</p>
  </div>
{/snippet}

<BrandTraces />
<div class="solo-card auth-card" class:wide={flow === 'setup'}>
  <div class="brand"><Logo tagline /></div>
  <section class="panel auth" aria-labelledby="auth-heading" aria-busy={busy}>
    {#if flow === 'setup'}
      <ol class="steps" aria-label={tr('admSetupSteps')}>
        <li aria-current={step === 0 ? 'step' : undefined} data-done={step > 0 || undefined}>
          {tr('setupToken')}{#if step > 0}<span class="visually-hidden">, {tr('admStepCompleted')}</span>{/if}
        </li>
        <li aria-current={step === 1 ? 'step' : undefined}>{tr('admStepAdministrator')}</li>
      </ol>
    {/if}
    <header>
      <h1 id="auth-heading" tabindex="-1" bind:this={heading}>{title}</h1>
      <p class="muted">{intro}</p>
    </header>

    {#if flow === 'setup' && step === 0}
      <form onsubmit={continueSetup} novalidate>
        <pre class="cmd"><span class="muted" aria-hidden="true">$ </span>shortcircuit setup-token</pre>
        {@render tokenField(tr('setupToken'))}
        <button class="btn" data-variant="primary" data-size="lg" type="submit">{tr('admContinue')}</button>
      </form>
    {:else}
      <form onsubmit={submit} novalidate>
        {#if notice && flow === 'login'}
          <div class="notice" data-tone="warning" role="status"><Icon name="info" /><div><p>{locale.t[notice]}</p></div></div>
        {/if}
        {#if formError}
          <div class="notice" data-tone="error" role="alert" tabindex="-1" bind:this={errorBox}><Icon name="circle-alert" /><div><p>{locale.t[formError]}</p></div></div>
        {/if}
        {#if flow === 'recover'}
          <pre class="cmd"><span class="muted" aria-hidden="true">$ </span>shortcircuit recovery-token</pre>
          {@render tokenField(tr('recoveryToken'))}
        {:else}
          <div class="field">
            <label class="field-label" for="auth-username">{tr('username')}</label>
            <input
              id="auth-username"
              class="input"
              name="username"
              autocomplete="username"
              autocapitalize="none"
              spellcheck="false"
              bind:value={username}
              disabled={busy}
              aria-invalid={errorFor('username') ? 'true' : undefined}
              aria-describedby={described(!!errorFor('username') && 'auth-username-error')}
            />
            {@render error('username')}
          </div>
        {/if}
        <div class="field">
          <label class="field-label" for="auth-password">{flow === 'recover' ? tr('newPassword') : tr('password')}</label>
          <input
            id="auth-password"
            class="input"
            type="password"
            name="password"
            autocomplete={flow === 'login' ? 'current-password' : 'new-password'}
            bind:value={password}
            disabled={busy}
            aria-invalid={errorFor('password') ? 'true' : undefined}
            aria-describedby={described(!!errorFor('password') && 'auth-password-error', flow !== 'login' && 'auth-password-rule', flow === 'recover' && 'auth-password-sessions')}
          />
          {@render error('password')}
          {#if flow !== 'login'}
            <p class="field-hint rule" id="auth-password-rule" data-ok={passwordOk || undefined}>
              {#if passwordOk}<Icon name="check" />{/if}{length ? tr('admPasswordRuleCount', { count: length }) : tr('admPasswordRule')}
            </p>
          {/if}
          {#if flow === 'recover'}<p class="field-hint" id="auth-password-sessions">{tr('admRecoverSessions')}</p>{/if}
        </div>
        <button class="btn" data-variant="primary" data-size="lg" type="submit" disabled={busy} aria-busy={busy}>
          {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
          <span>{flow === 'setup' ? tr('createAccount') : flow === 'recover' ? tr('admSetNewPassword') : tr('signIn')}</span>
        </button>
      </form>
    {/if}
  </section>

  {#if flow !== 'setup'}
    <button class="btn switch-flow" data-variant="link" type="button" disabled={busy} onclick={() => switchFlow(!recover)}>
      {recover ? tr('backToSignIn') : tr('admForgotPassword')}
    </button>
  {:else if step === 1}
    <button class="btn switch-flow" data-variant="link" type="button" disabled={busy} onclick={() => goStep(0)}>{tr('admBackToToken')}</button>
  {/if}

  <div class="display" role="group" aria-label={tr('admDisplayOptions')}>
    <select class="select" data-size="sm" aria-label={tr('language')} value={locale.language} onchange={(e) => setLanguage(e.currentTarget.value as Language)}>
      <option value="en" lang="en">English</option>
      <option value="de" lang="de">Deutsch</option>
    </select>
    <Menu label={tr('themeLabel')} icon={theme.choice === 'system' ? 'monitor' : theme.choice === 'dark' ? 'moon' : 'sun'} size="sm" width={180}>
      {#snippet children(close)}
        <div class="menu-label">{tr('themeLabel')}</div>
        {#each [['dark', 'moon', 'themeDark'], ['light', 'sun', 'themeLight'], ['system', 'monitor', 'themeSystem']] as const as [value, icon, label] (value)}
          <button class="menu-item" role="menuitemradio" aria-checked={theme.choice === value} onclick={() => { setTheme(value); close(); }}>
            <Icon name={icon} />{tr(label)}
          </button>
        {/each}
      {/snippet}
    </Menu>
  </div>
</div>

<style>
  .auth-card {
    position: relative;
    z-index: 1;
  }
  .auth-card.wide {
    width: min(440px, 100%);
  }
  .brand {
    justify-self: center;
  }
  .auth {
    display: grid;
    gap: var(--space-6);
    padding: var(--space-8);
  }
  header {
    display: grid;
    gap: 6px;
  }
  h1 {
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
  }
  header p {
    font-size: var(--text-sm);
  }
  form {
    display: grid;
    gap: var(--space-4);
  }
  form > .btn {
    width: 100%;
    margin-top: var(--space-2);
  }
  .steps {
    display: flex;
    gap: var(--space-2);
    margin-bottom: calc(var(--space-1) * -1);
  }
  .steps li {
    flex: 1;
    padding-top: 10px;
    border-top: 2px solid var(--border);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
  }
  .steps li[aria-current='step'] {
    border-color: var(--accent);
    color: var(--text);
  }
  .steps li[data-done] {
    border-color: var(--success);
  }
  .cmd {
    margin: 0;
    padding: 12px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    overflow-x: auto;
  }
  .rule {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .rule[data-ok] {
    color: var(--success);
  }
  .rule :global(svg) {
    width: 13px;
    height: 13px;
  }
  .switch-flow {
    justify-self: center;
    font-size: var(--text-sm);
  }
  .display {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: var(--space-2);
  }
  .display .select {
    width: auto;
  }
  @media (max-width: 480px) {
    .auth {
      padding: var(--space-6) var(--space-5);
    }
  }
</style>
