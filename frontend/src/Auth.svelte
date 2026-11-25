<script lang="ts">
  import { tick } from 'svelte';
  import { ApiError } from './api';
  import type { Api, User } from './api';
  import { labelAllowed, passwordAllowed, problemMessage } from './format';
  import type { Messages } from './i18n';

  let { t, setup, api, notice, signedIn, refreshSession }: {
    t: Messages;
    setup: boolean;
    api: Api;
    notice: keyof Messages | null;
    signedIn: (user: User) => void;
    refreshSession: () => Promise<void>;
  } = $props();

  let recover = $state(false);
  let token = $state('');
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let error = $state<keyof Messages | null>(null);
  let heading: HTMLHeadingElement;
  const flow = $derived(setup ? 'setup' : recover ? 'recover' : 'login');

  async function switchFlow(next: boolean) {
    recover = next;
    error = null;
    token = '';
    password = '';
    await tick();
    heading.focus();
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    error = null;
    const name = username.trim();
    if (flow === 'setup' && !labelAllowed(name)) {
      error = 'invalidUsername';
      return;
    }
    if (flow !== 'login' && !passwordAllowed(password)) {
      error = 'invalidPassword';
      return;
    }

    const payload = flow === 'login' ? { username: name, password }
      : flow === 'setup' ? { token: token.trim(), username: name, password }
        : { token: token.trim(), password };
    busy = true;
    try {
      signedIn(await api<User>(`/auth/${flow}`, 'POST', payload));
      token = '';
      password = '';
    } catch (failure) {
      if (flow === 'setup' && failure instanceof ApiError && failure.code === 'auth_conflict') {
        await refreshSession();
      } else {
        error = problemMessage(failure, flow === 'login' ? 'invalidCredentials' : 'invalidToken');
      }
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel auth-card" aria-labelledby="auth-heading">
  <div class="brand"><img src="/favicon.svg" alt="" /> ShortCircuit</div>
  <div class="stack compact">
    <h1 id="auth-heading" tabindex="-1" bind:this={heading}>{flow === 'setup' ? t.setup : flow === 'recover' ? t.recover : t.signIn}</h1>
    {#if flow === 'setup'}<p>{t.setupHint}</p>{/if}
    {#if flow === 'recover'}<p>{t.recoveryHint}</p>{/if}
  </div>
  <form class="stack auth-form" onsubmit={submit} aria-busy={busy}>
    {#if notice && !recover}<p class="notice" role="status">{t[notice]}</p>{/if}
    {#if flow !== 'login'}
      <label class="field">
        <span>{flow === 'setup' ? t.setupToken : t.recoveryToken}</span>
        <input name="token" type="password" autocomplete="off" spellcheck="false" required bind:value={token} disabled={busy} />
      </label>
    {/if}
    {#if flow !== 'recover'}
      <label class="field">
        <span>{t.username}</span>
        <input name="username" autocomplete="username" autocapitalize="none" spellcheck="false" required bind:value={username} disabled={busy} />
      </label>
    {/if}
    <label class="field">
      <span id="auth-password-label">{flow === 'recover' ? t.newPassword : t.password}</span>
      <input name="password" type="password" autocomplete={flow === 'login' ? 'current-password' : 'new-password'} required bind:value={password} disabled={busy} aria-labelledby="auth-password-label" aria-invalid={error === 'invalidPassword'} aria-describedby={flow === 'login' ? undefined : 'auth-password-hint'} />
      {#if flow !== 'login'}<small id="auth-password-hint">{t.passwordHint}</small>{/if}
    </label>
    {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
    <button class="button primary" type="submit" disabled={busy}>
      {busy ? t.saving : flow === 'setup' ? t.createAccount : flow === 'recover' ? t.recover : t.signIn}
    </button>
  </form>
  {#if flow !== 'setup'}
    <button class="text-button" type="button" disabled={busy} onclick={() => switchFlow(!recover)}>
      {recover ? t.backToSignIn : t.recover}
    </button>
  {/if}
</section>
