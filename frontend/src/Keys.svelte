<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, scopes } from './api';
  import type { Api, Domains, Key, Scope } from './api';
  import Dialog from './Dialog.svelte';
  import { formatDate, fromUnixSeconds, labelAllowed, problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';
  import { useTimeZone } from './timezone';

  const readTimeZone = useTimeZone();

  let { t, language, api, domains }: { t: Messages; language: Language; api: Api; domains: Domains } = $props();
  const scopeLabels: Record<Scope, keyof Messages> = { read: 'read', write: 'write', delete: 'deleteScope' };
  let items = $state<Key[]>([]);
  let loading = $state(true);
  let loadError = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);
  let adding = $state(false);
  let removing = $state<Key | null>(null);
  let rotating = $state<Key | null>(null);
  let currentPassword = $state('');
  let saving = $state(false);
  let formError = $state<keyof Messages | null>(null);
  let rotateError = $state<keyof Messages | null>(null);
  let name = $state('');
  let expiration = $state('');
  let host = $state('');
  let permissions = $state<Scope[]>([]);
  let token = $state('');
  let replacementToken = $state(false);
  let copyMessage = $state<'copiedToken' | 'copyFailed' | null>(null);

  onMount(() => { void load(); });

  async function load() {
    loading = true;
    loadError = null;
    try {
      const result = await api<{ items: Key[] }>('/keys');
      items = result.items;
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  function openForm() {
    name = '';
    host = '';
    expiration = '';
    permissions = [];
    formError = null;
    adding = true;
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    formError = null;
    if (!labelAllowed(name.trim())) {
      formError = 'invalidKeyName';
      return;
    }
    if (!permissions.length) {
      formError = 'invalidScopes';
      return;
    }
    const date = expiration ? new Date(expiration) : null;
    if (date && (Number.isNaN(date.getTime()) || date.getTime() <= Date.now())) {
      formError = 'invalidExpiration';
      return;
    }
    saving = true;
    notice = null;
    try {
      const result = await api<{ key: Key; token: string }>('/keys', 'POST', {
        name: name.trim(),
        scopes: permissions,
        ...(host ? { host } : {}),
        ...(date ? { expires_at: Math.floor(date.getTime() / 1000) } : {}),
      });
      items = [result.key, ...items.filter((key) => key.id !== result.key.id)];
      adding = false;
      copyMessage = null;
      replacementToken = false;
      token = result.token;
    } catch (error) {
      formError = problemMessage(error);
    } finally {
      saving = false;
    }
  }

  function closeRotation() {
    rotating = null;
    currentPassword = '';
  }

  async function rotate(event: SubmitEvent) {
    event.preventDefault();
    if (!rotating || saving) return;
    const id = rotating.id;
    saving = true;
    rotateError = null;
    notice = null;
    try {
      const result = await api<{ key: Key; token: string }>(`/keys/${id}/rotate`, 'POST', {
        current_password: currentPassword,
      });
      items = items.map((key) => key.id === id ? result.key : key);
      closeRotation();
      copyMessage = null;
      replacementToken = true;
      token = result.token;
    } catch (error) {
      if (error instanceof ApiError && error.code === 'expired_key') rotateError = 'expiredKey';
      else if (error instanceof ApiError && error.code === 'invalid_credentials') rotateError = 'incorrectCurrentPassword';
      else rotateError = problemMessage(error);
    } finally {
      currentPassword = '';
      saving = false;
    }
  }

  async function revoke(event: SubmitEvent) {
    event.preventDefault();
    if (!removing || saving) return;
    saving = true;
    formError = null;
    notice = null;
    try {
      await api<void>(`/keys/${removing.id}`, 'DELETE');
      items = items.filter((key) => key.id !== removing?.id);
      removing = null;
      notice = 'keyRevoked';
    } catch (error) {
      formError = problemMessage(error);
    } finally {
      saving = false;
    }
  }

  async function copyToken() {
    copyMessage = null;
    try {
      await navigator.clipboard.writeText(token);
      copyMessage = 'copiedToken';
    } catch {
      copyMessage = 'copyFailed';
    }
  }

  function scopeLabel(scope: string): string {
    return scopes.includes(scope as Scope) ? t[scopeLabels[scope as Scope]] : scope;
  }
</script>

<div class="page-head">
  <div><h1>{t.apiKeys}</h1><p>{t.keysDescription}</p></div>
  <button class="button primary" type="button" onclick={openForm} disabled={loading || !!loadError}><Icon name="plus" />{t.addKey}</button>
</div>

{#if notice}<p class="notice page-notice" role="status">{t[notice]}</p>{/if}

<section class="panel" aria-label={t.apiKeys} aria-busy={loading}>
  {#if items.length}
    <div class="table-wrap">
      <!-- svelte-ignore a11y_no_redundant_roles (Mobile card layout overrides native table display.) -->
      <table class="keys-table" role="table" aria-label={t.apiKeys}>
        <thead>
          <tr><th scope="col">{t.keyName}</th><th scope="col">{t.scopes}</th><th scope="col">{t.domain}</th><th scope="col">{t.expires}</th><th scope="col">{t.created}</th><th scope="col" class="align-right">{t.actions}</th></tr>
        </thead>
        <!-- svelte-ignore a11y_no_redundant_roles (Preserve table semantics in mobile cards.) -->
        <tbody role="rowgroup">
          {#each items as key (key.id)}
            {@const createdTime = fromUnixSeconds(key.created_at)}
            {@const expired = key.expires_at !== null && key.expires_at * 1000 <= Date.now()}
            <!-- svelte-ignore a11y_no_redundant_roles (Body rows display as grids on mobile.) -->
            <tr role="row">
              <td role="cell"><strong>{key.name}</strong></td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.scopes}</span><div class="tags">{#each key.scopes as scope (scope)}<span class="tag">{scopeLabel(scope)}</span>{/each}</div></td>
              <td role="cell"><span class="cell-label" aria-hidden="true">{t.domain}</span>{key.host || t.allDomains}</td>
              <td role="cell">
                <span class="cell-label" aria-hidden="true">{t.expires}</span>
                {#if key.expires_at !== null}
                  {@const expirationTime = fromUnixSeconds(key.expires_at)}
                  {#if expirationTime}
                    <time datetime={expirationTime}>{formatDate(expirationTime, language, true, readTimeZone())}</time>
                  {:else}
                    {t.dateOutOfRange}
                  {/if}
                  {#if expired}<span class="status expired">{t.expired}</span>{/if}
                {:else}{t.never}{/if}
              </td>
              <td role="cell">
                <span class="cell-label" aria-hidden="true">{t.created}</span>
                {#if createdTime}<time datetime={createdTime}>{formatDate(createdTime, language, false, readTimeZone())}</time>{:else}{t.dateOutOfRange}{/if}
              </td>
              <td role="cell" class="align-right">
                <div class="row row-actions key-actions">
                  <button class="button" type="button" disabled={expired} onclick={() => { rotateError = null; rotating = key; }}>{t.rotateKey}</button>
                  <button class="button danger" type="button" onclick={() => { formError = null; removing = key; }}>{t.revoke}</button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !loading && !loadError}
    <div class="empty"><span class="link-symbol"><Icon name="key" /></span><p>{t.noKeys}</p></div>
  {/if}
  {#if loading}<div class="panel-foot"><p role="status">{t.loading}</p></div>{/if}
  {#if loadError}<div class="panel-foot between"><p class="notice error" role="alert">{t[loadError]}</p><button class="button" type="button" onclick={load}>{t.retry}</button></div>{/if}
</section>

<Dialog {t} title={t.addKey} open={adding} busy={saving} onclose={() => adding = false}>
  <form onsubmit={create} aria-busy={saving}>
    <div class="dialog-body stack">
      <label class="field"><span>{t.keyName}</span><input name="name" required bind:value={name} disabled={saving} aria-invalid={formError === 'invalidKeyName'} /></label>
      <fieldset class="permissions">
        <legend>{t.scopes}</legend>
        {#each scopes as scope (scope)}
          <label><input type="checkbox" name="scopes" value={scope} bind:group={permissions} disabled={saving} />{scopeLabel(scope)}</label>
        {/each}
      </fieldset>
      <label class="field">
        <span>{t.domain}</span>
        <select name="host" bind:value={host} disabled={saving}>
          <option value="">{t.allDomains}</option>
          {#each domains.hosts as value (value)}<option value={value}>{value}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span id="expiration-label">{t.expiresOptional}</span>
        <input type="datetime-local" name="expires_at" bind:value={expiration} disabled={saving} aria-labelledby="expiration-label" aria-describedby="expiration-hint" aria-invalid={formError === 'invalidExpiration'} />
        <small id="expiration-hint">{t.expirationHint}</small>
      </label>
      {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
    </div>
    <footer class="dialog-foot row">
      <button class="button" type="button" disabled={saving} onclick={() => adding = false}>{t.cancel}</button>
      <button class="button primary" type="submit" disabled={saving}>{saving ? t.saving : t.addKey}</button>
    </footer>
  </form>
</Dialog>

<Dialog {t} title={replacementToken ? t.tokenRotated : t.tokenCreated} open={!!token} onclose={() => token = ''}>
  <div class="dialog-body stack">
    <p>{t.tokenHint}</p>
    <label class="field"><span>{t.apiToken}</span><input class="mono" readonly value={token} spellcheck="false" autocomplete="off" /></label>
    {#if copyMessage}<p class:error={copyMessage === 'copyFailed'} class="notice" role="status">{t[copyMessage]}</p>{/if}
  </div>
  <footer class="dialog-foot row">
    <button class="button" type="button" onclick={() => token = ''}>{t.close}</button>
    <button class="button primary" type="button" onclick={copyToken}><Icon name="copy" />{t.copyToken}</button>
  </footer>
</Dialog>

<Dialog {t} title={t.revokeKey} open={removing !== null} busy={saving} onclose={() => removing = null}>
  {#if removing}
    <form onsubmit={revoke} aria-busy={saving}>
      <div class="dialog-body stack compact">
        <strong>{removing.name}</strong>
        <p>{t.revokeHint}</p>
        {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
      </div>
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={saving} onclick={() => removing = null}>{t.cancel}</button>
        <button class="button danger" type="submit" disabled={saving}>{saving ? t.saving : t.revoke}</button>
      </footer>
    </form>
  {/if}
</Dialog>

<Dialog {t} title={t.rotateKey} open={rotating !== null} busy={saving} onclose={closeRotation}>
  {#if rotating}
    <form onsubmit={rotate} aria-busy={saving}>
      <div class="dialog-body stack">
        <strong>{rotating.name}</strong>
        <p>{t.rotateHint}</p>
        <label class="field">
          <span>{t.currentPassword}</span>
          <input name="current_password" type="password" autocomplete="current-password" required bind:value={currentPassword} disabled={saving} />
        </label>
        {#if rotateError}<p class="notice error" role="alert">{t[rotateError]}</p>{/if}
      </div>
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={saving} onclick={closeRotation}>{t.cancel}</button>
        <button class="button danger" type="submit" disabled={saving}>{saving ? t.saving : t.rotateKey}</button>
      </footer>
    </form>
  {/if}
</Dialog>

<style>
  .keys-table th:last-child { width: 140px; }
  .key-actions {
    gap: 6px;
  }
</style>
