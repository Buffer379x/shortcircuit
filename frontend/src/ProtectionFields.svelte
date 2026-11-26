<script lang="ts">
  import type { Messages } from './i18n';

  let {
    t, oneTime = $bindable(), password = $bindable(), removePassword = $bindable(false),
    hasPassword = false, busy = false, invalidPassword = false,
  }: {
    t: Messages;
    oneTime: boolean;
    password: string;
    removePassword?: boolean;
    hasPassword?: boolean;
    busy?: boolean;
    invalidPassword?: boolean;
  } = $props();

  const fieldId = $props.id();
</script>

<div class="stack compact" role="group" aria-label={t.linkProtection}>
  <label class="row check-row">
    <input type="checkbox" bind:checked={oneTime} disabled={busy} aria-describedby={`${fieldId}-one-time`} />
    {t.oneTimeLink}
  </label>
  <small id={`${fieldId}-one-time`}>{t.oneTimeLinkHint}</small>
  {#if hasPassword}<p>{t.linkPasswordEnabled}</p>{/if}
  <label class="field">
    <span id={`${fieldId}-password-label`}>{hasPassword ? t.newPassword : t.linkPasswordOptional}</span>
    <input type="password" autocomplete="new-password" bind:value={password} disabled={busy || removePassword}
      aria-invalid={invalidPassword} aria-labelledby={`${fieldId}-password-label`} aria-describedby={`${fieldId}-password`} />
    <small id={`${fieldId}-password`}>
      {#if hasPassword}{t.keepLinkPassword} {/if}{t.passwordHint}
    </small>
  </label>
  {#if hasPassword}
    <label class="row check-row">
      <input type="checkbox" bind:checked={removePassword} disabled={busy}
        onchange={(event) => { if (event.currentTarget.checked) password = ''; }} />
      {t.removeLinkPassword}
    </label>
  {/if}
</div>

<style>
  .check-row { min-height: 32px; }
</style>
