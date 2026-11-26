<script lang="ts">
  import type { Api, User } from './api';
  import { passwordAllowed, problemMessage } from './format';
  import type { Messages } from './i18n';
  import { defaultTimeZone } from './timezone';

  let { t, user, api, timeZone = $bindable(), onPasswordChanged }: {
    t: Messages;
    user: User;
    api: Api;
    timeZone: string | undefined;
    onPasswordChanged: (user?: User) => Promise<void>;
  } = $props();
  const timeZones = [...new Set(['UTC', defaultTimeZone(), ...(Intl.supportedValuesOf?.('timeZone') ?? [])])];
  const options = $derived(timeZone && !timeZones.includes(timeZone) ? [...timeZones, timeZone] : timeZones);
  let current = $state('');
  let password = $state('');
  let saving = $state(false);
  let changed = $state(false);
  let error = $state<keyof Messages | null>(null);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    changed = false;
    error = null;
    if (!passwordAllowed(password)) {
      error = 'invalidPassword';
      return;
    }
    saving = true;
    try {
      const updated = await api<User | undefined>('/auth/password', 'POST', { current_password: current, password });
      current = '';
      password = '';
      await onPasswordChanged(updated);
      changed = true;
    } catch (failure) {
      error = problemMessage(failure, 'invalidCredentials');
    } finally {
      saving = false;
    }
  }
</script>

<div class="page-head"><div><h1>{t.account}</h1><p>{user.username}</p></div></div>
<section class="panel account-panel" aria-labelledby="password-heading">
  <h2 id="password-heading">{t.changePassword}</h2>
  <form class="stack" onsubmit={submit} aria-busy={saving}>
    <label class="field">
      <span>{t.currentPassword}</span>
      <input name="current_password" required type="password" autocomplete="current-password" bind:value={current} disabled={saving} />
    </label>
    <label class="field">
      <span id="account-password-label">{t.newPassword}</span>
      <input name="password" required type="password" autocomplete="new-password" bind:value={password} disabled={saving} aria-labelledby="account-password-label" aria-describedby="account-password-hint" aria-invalid={error === 'invalidPassword'} />
      <small id="account-password-hint">{t.passwordHint}</small>
    </label>
    {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
    {#if changed}<p class="notice" role="status">{t.passwordChanged}</p>{/if}
    <div><button class="button primary" type="submit" disabled={saving}>{saving ? t.saving : t.changePassword}</button></div>
  </form>
</section>


<section class="panel account-panel stack" aria-labelledby="display-heading">
  <h2 id="display-heading">{t.displayPreferences}</h2>
  <label class="field">
    <span>{t.displayTimezone}</span>
    <select bind:value={timeZone} aria-describedby="display-timezone-hint">
      <option value={undefined}>{t.browserTimezone} · {defaultTimeZone()}</option>
      {#each options as name (name)}
        <option value={name}>{name.replaceAll('_', ' ')}</option>
      {/each}
    </select>
    <small id="display-timezone-hint">{t.displayTimezoneHint}</small>
  </label>
</section>
