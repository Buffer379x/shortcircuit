<script lang="ts">
  // API keys: create with scopes, domain, and expiry; show the secret once; rotate with the
  // administrator password; revoke with confirmation.
  import { onMount, tick } from 'svelte';
  import { ApiError, scopes } from '../api';
  import type { Key, Scope } from '../api';
  import { labelAllowed, problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import { app } from '../lib/app.svelte';
  import { date } from '../lib/fmt';
  import { useTimeZone } from '../timezone';
  import { locale, tr } from '../lib/locale.svelte';
  import { toast } from '../lib/toast.svelte';

  const scopeInfo: Record<Scope, [keyof Messages, keyof Messages]> = {
    read: ['admScopeRead', 'read'],
    write: ['admScopeWrite', 'write'],
    delete: ['admScopeDelete', 'deleteScope'],
    analytics: ['admScopeAnalytics', 'analyticsScope'],
  };
  const presets = ['30', '90', '365', 'never', 'custom'] as const;
  type Preset = typeof presets[number];
  const presetLabels: Record<Preset, keyof Messages> = {
    '30': 'admExpires30', '90': 'admExpires90', '365': 'admExpires365', never: 'never', custom: 'admExpiresCustom',
  };

  let items = $state<Key[]>([]);
  let loading = $state(true);
  let loadError = $state<keyof Messages | null>(null);
  let heading = $state<HTMLElement>();

  let createOpen = $state(false);
  let name = $state('');
  let permissions = $state<Scope[]>([]);
  let host = $state('');
  let preset = $state<Preset>('365');
  let expiration = $state('');
  let saving = $state(false);
  let fieldError = $state<{ field: 'name' | 'scopes' | 'expires'; message: keyof Messages } | null>(null);
  let formError = $state<keyof Messages | null>(null);

  let secret = $state('');
  let replacement = $state(false);
  let secretOpen = $state(false);

  let rotating = $state<Key | null>(null);
  let rotateOpen = $state(false);
  let currentPassword = $state('');
  let rotateError = $state<keyof Messages | null>(null);

  let removing = $state<Key | null>(null);
  let revokeOpen = $state(false);
  let revokeError = $state<keyof Messages | null>(null);

  const readTimeZone = useTimeZone();
  const now = Date.now();
  const stamp = (seconds: number) =>
    new Intl.DateTimeFormat(locale.language, { dateStyle: 'medium', timeStyle: 'short', timeZone: readTimeZone() }).format(seconds * 1000);
  const apiBase = `${location.origin}/api/v1`;

  onMount(() => {
    heading = document.querySelector<HTMLElement>('main h1') ?? undefined;
    void load();
  });

  async function load() {
    loading = true;
    loadError = null;
    try {
      items = (await app.api<{ items: Key[] }>('/keys')).items;
      app.counts.keys = items.length;
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  function openCreate() {
    name = '';
    host = '';
    preset = '365';
    expiration = '';
    permissions = ['read'];
    fieldError = null;
    formError = null;
    createOpen = true;
  }

  function errorFor(field: 'name' | 'scopes' | 'expires') {
    return fieldError?.field === field ? fieldError.message : null;
  }

  async function fail(field: 'name' | 'scopes' | 'expires', message: keyof Messages, focus: string) {
    fieldError = { field, message };
    await tick();
    document.querySelector<HTMLElement>(focus)?.focus();
  }

  function expiresAt(): number | null | 'invalid' {
    if (preset === 'never') return null;
    if (preset !== 'custom') return Math.floor(Date.now() / 1000) + Number(preset) * 86_400;
    if (!expiration) return 'invalid';
    const value = new Date(expiration);
    if (Number.isNaN(value.getTime()) || value.getTime() <= Date.now()) return 'invalid';
    return Math.floor(value.getTime() / 1000);
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    fieldError = null;
    formError = null;
    if (!labelAllowed(name.trim())) return fail('name', 'invalidKeyName', '#key-name');
    if (!permissions.length) return fail('scopes', 'invalidScopes', '#key-scopes input');
    const expires = expiresAt();
    if (expires === 'invalid') return fail('expires', 'invalidExpiration', preset === 'custom' ? '#key-expires-at' : '#key-expires');
    saving = true;
    try {
      const result = await app.api<{ key: Key; token: string }>('/keys', 'POST', {
        name: name.trim(),
        scopes: permissions,
        ...(host ? { host } : {}),
        ...(expires !== null ? { expires_at: expires } : {}),
      });
      items = [result.key, ...items.filter((key) => key.id !== result.key.id)];
      app.counts.keys = items.length;
      createOpen = false;
      showSecret(result.token, false);
    } catch (error) {
      const message = problemMessage(error);
      if (message === 'invalidScopes') await fail('scopes', message, '#key-scopes input');
      else if (message === 'invalidExpiration') await fail('expires', message, '#key-expires');
      else formError = message;
    } finally {
      saving = false;
    }
  }

  async function showSecret(token: string, rotated: boolean) {
    replacement = rotated;
    secret = token;
    // Open after the previous dialog closed, so focus returns to the page afterwards.
    await tick();
    secretOpen = true;
  }

  function closeSecret() {
    secret = '';
    secretOpen = false;
    heading?.focus();
  }

  function openRotate(key: Key) {
    rotating = key;
    currentPassword = '';
    rotateError = null;
    rotateOpen = true;
  }

  function closeRotate() {
    rotateOpen = false;
    currentPassword = '';
  }

  async function rotate(event: SubmitEvent) {
    event.preventDefault();
    if (!rotating || saving) return;
    const id = rotating.id;
    if (!currentPassword) {
      rotateError = 'admPasswordRequired';
      await tick();
      document.getElementById('key-password')?.focus();
      return;
    }
    saving = true;
    rotateError = null;
    let failed = false;
    try {
      const result = await app.api<{ key: Key; token: string }>(`/keys/${id}/rotate`, 'POST', { current_password: currentPassword });
      items = items.map((key) => (key.id === id ? result.key : key));
      rotateOpen = false;
      void showSecret(result.token, true);
    } catch (error) {
      failed = true;
      if (error instanceof ApiError && error.code === 'expired_key') rotateError = 'expiredKey';
      else if (error instanceof ApiError && error.code === 'invalid_credentials') rotateError = 'incorrectCurrentPassword';
      else rotateError = problemMessage(error);
    } finally {
      currentPassword = '';
      saving = false;
    }
    if (failed) {
      await tick();
      document.getElementById('key-password')?.focus();
    }
  }

  function openRevoke(key: Key) {
    removing = key;
    revokeError = null;
    revokeOpen = true;
  }

  async function revoke() {
    if (!removing || saving) return;
    const key = removing;
    saving = true;
    revokeError = null;
    try {
      await app.api<void>(`/keys/${key.id}`, 'DELETE');
      items = items.filter((item) => item.id !== key.id);
      app.counts.keys = items.length;
      revokeOpen = false;
      toast(tr('keyRevoked'), { body: key.name });
      await tick();
      heading?.focus();
    } catch (error) {
      revokeError = problemMessage(error);
    } finally {
      saving = false;
    }
  }

  function safe(format: (value: number) => string, value: number) {
    try {
      return format(value);
    } catch {
      return tr('dateOutOfRange');
    }
  }

  function scopeLabel(scope: string) {
    return Object.hasOwn(scopeInfo, scope) ? tr(scopeInfo[scope as Scope][0]) : scope;
  }

  function expiry(key: Key) {
    if (key.expires_at === null) return 'never' as const;
    const ms = key.expires_at * 1000;
    if (ms <= now) return 'expired' as const;
    return ms - now < 30 * 86_400_000 ? ('soon' as const) : ('later' as const);
  }
</script>

<div class="page">
  <PageHeader title={tr('apiKeys')} description={tr('admKeysDescription')}>
    {#snippet actions()}
      <button class="btn" data-variant="primary" disabled={loading || !!loadError} onclick={openCreate}><Icon name="plus" />{tr('addKey')}</button>
    {/snippet}
  </PageHeader>

  <section class="panel" aria-label={tr('apiKeys')} aria-busy={loading}>
    {#if loadError}
      <div class="panel-body">
        <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[loadError]}</p></div><button class="btn" data-size="sm" onclick={load}>{tr('tryAgain')}</button></div>
      </div>
    {:else if loading}
      <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
    {:else if !items.length}
      <Empty icon="key-round" title={tr('noKeys')}>{tr('admNoKeysHint')}</Empty>
    {:else}
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th scope="col">{tr('keyName')}</th>
              <th scope="col">{tr('scopes')}</th>
              <th scope="col">{tr('domain')}</th>
              <th scope="col">{tr('created')}</th>
              <th scope="col">{tr('expires')}</th>
              <th scope="col" class="c-actions"><span class="visually-hidden">{tr('actions')}</span></th>
            </tr>
          </thead>
          <tbody>
            {#each items as key (key.id)}
              {@const state = expiry(key)}
              <tr>
                <td><b class="key-name">{key.name}</b></td>
                <td>
                  <div class="scopes">
                    {#each key.scopes as scope (scope)}<span class="badge" data-tone={scope === 'delete' ? 'warning' : 'neutral'}>{scopeLabel(scope)}</span>{/each}
                  </div>
                </td>
                <td>{#if key.host}<span class="mono small">{key.host}</span>{:else}<span class="muted">{tr('admAllDomains')}</span>{/if}</td>
                <td class="nowrap">{safe(date, key.created_at)}</td>
                <td class="nowrap">
                  {#if state === 'never'}<span class="muted">{tr('never')}</span>
                  {:else if state === 'expired'}<Badge state={{ tone: 'neutral', icon: 'hourglass', label: tr('expired') }} />
                  {:else}<span class:warn={state === 'soon'}>{#if state === 'soon'}<Icon name="clock" />{/if}{safe(stamp, key.expires_at ?? 0)}</span>{/if}
                </td>
                <td class="c-actions">
                  <Menu label={tr('admKeyActions', { name: key.name })} icon="ellipsis" size="sm">
                    {#snippet children(close)}
                      <button class="menu-item" role="menuitem" disabled={state === 'expired'} onclick={() => { close(); openRotate(key); }}><Icon name="refresh-cw" />{tr('rotateKey')}</button>
                      <div class="menu-sep"></div>
                      <button class="menu-item" role="menuitem" data-tone="danger" onclick={() => { close(); openRevoke(key); }}><Icon name="ban" />{tr('revoke')}</button>
                    {/snippet}
                  </Menu>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <div class="notice" data-tone="neutral">
    <Icon name="file-text" />
    <div><b>{tr('admApiReference')}</b><p>{tr('admApiReferenceText1')} <code>/api/v1/openapi.json</code> {tr('admApiReferenceText2')} <code>Authorization: Bearer &lt;key&gt;</code>.</p></div>
  </div>
</div>

{#snippet err(field: 'name' | 'scopes' | 'expires')}
  {@const message = errorFor(field)}
  {#if message}<p class="field-error" id="key-{field}-error"><Icon name="circle-alert" />{locale.t[message]}</p>{/if}
{/snippet}

<Dialog bind:open={createOpen} title={tr('addKey')} width={540} busy={saving}>
  <form id="key-form" class="form" onsubmit={create} novalidate aria-busy={saving}>
    <div class="field">
      <label class="field-label" for="key-name">{tr('keyName')}</label>
      <input id="key-name" class="input" bind:value={name} disabled={saving} maxlength="64" placeholder={tr('admKeyNamePlaceholder')} autocomplete="off" aria-invalid={errorFor('name') ? 'true' : undefined} aria-describedby={errorFor('name') ? 'key-name-error' : undefined} />
      {@render err('name')}
    </div>
    <fieldset id="key-scopes" class="fieldset" disabled={saving} aria-describedby={errorFor('scopes') ? 'key-scopes-error key-scopes-hint' : 'key-scopes-hint'}>
      <legend>{tr('scopes')}</legend>
      <div class="choice-grid">
        {#each scopes as scope (scope)}
          <label class="choice"><input class="checkbox" type="checkbox" value={scope} bind:group={permissions} /><span><b>{tr(scopeInfo[scope][0])}</b><small>{tr(scopeInfo[scope][1])}</small></span></label>
        {/each}
      </div>
      {@render err('scopes')}
      <p class="field-hint" id="key-scopes-hint">{tr('scopesHint')}</p>
    </fieldset>
    <div class="field">
      <label class="field-label" for="key-host">{tr('domain')}</label>
      <select id="key-host" class="select" bind:value={host} disabled={saving} aria-describedby="key-host-hint">
        <option value="">{tr('allDomains')}</option>
        {#each app.domains?.hosts ?? [] as value (value)}<option value={value}>{value}</option>{/each}
      </select>
      <p class="field-hint" id="key-host-hint">{tr('admKeyHostHint')}</p>
    </div>
    <div class="expires">
      <div class="field">
        <label class="field-label" for="key-expires">{tr('expires')}</label>
        <select id="key-expires" class="select" bind:value={preset} disabled={saving} aria-invalid={errorFor('expires') && preset !== 'custom' ? 'true' : undefined} aria-describedby={errorFor('expires') ? 'key-expires-error' : undefined}>
          {#each presets as value (value)}<option {value}>{tr(presetLabels[value])}</option>{/each}
        </select>
      </div>
      {#if preset === 'custom'}
        <div class="field">
          <label class="field-label" for="key-expires-at">{tr('admExpiresAt')}</label>
          <input id="key-expires-at" class="input" type="datetime-local" bind:value={expiration} disabled={saving} aria-invalid={errorFor('expires') ? 'true' : undefined} aria-describedby={errorFor('expires') ? 'key-expires-error key-expires-hint' : 'key-expires-hint'} />
        </div>
      {/if}
    </div>
    {@render err('expires')}
    {#if preset === 'custom'}<p class="field-hint" id="key-expires-hint">{tr('admExpiresLocal')}</p>{/if}
    {#if formError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[formError]}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={saving} onclick={() => (createOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="key-form" data-variant="primary" disabled={saving} aria-busy={saving}>
      {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{tr('addKey')}
    </button>
  {/snippet}
</Dialog>

<Dialog bind:open={secretOpen} title={replacement ? tr('tokenRotated') : tr('admCopyKeyTitle')} description={tr('admSecretOnce')} width={560} onclose={closeSecret}>
  <div class="secret">
    <input class="input mono" readonly value={secret} aria-label={tr('apiToken')} spellcheck="false" autocomplete="off" onfocus={(e) => e.currentTarget.select()} />
    <CopyButton value={secret} variant="primary" size="md" text={tr('copy')} label={tr('copyToken')} />
  </div>
  <div class="field">
    <span class="field-label">{tr('admTryIt')}</span>
    <pre class="code">curl -H "Authorization: Bearer {secret.slice(0, 12)}…" \
  {apiBase}/links</pre>
  </div>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" onclick={() => (secretOpen = false)}>{tr('admStoredKey')}</button>
  {/snippet}
</Dialog>

<Dialog bind:open={rotateOpen} title={tr('admRotateTitle', { name: rotating?.name ?? '' })} description={tr('rotateHint')} width={460} busy={saving} onclose={closeRotate}>
  <form id="key-rotate" class="form" onsubmit={rotate} novalidate aria-busy={saving}>
    <div class="field">
      <label class="field-label" for="key-password">{tr('currentPassword')}</label>
      <input id="key-password" class="input" type="password" autocomplete="current-password" bind:value={currentPassword} disabled={saving} aria-invalid={rotateError ? 'true' : undefined} aria-describedby={rotateError ? 'key-rotate-error' : 'key-rotate-hint'} />
      {#if rotateError}<p class="field-error" id="key-rotate-error" role="alert"><Icon name="circle-alert" />{locale.t[rotateError]}</p>{/if}
      <p class="field-hint" id="key-rotate-hint">{tr('admRotatePasswordHint')}</p>
    </div>
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={saving} onclick={closeRotate}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="key-rotate" data-variant="danger" disabled={saving} aria-busy={saving}>
      {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{tr('rotateKey')}
    </button>
  {/snippet}
</Dialog>

<Dialog bind:open={revokeOpen} title={tr('admRevokeTitle', { name: removing?.name ?? '' })} width={440} busy={saving}>
  <p class="secondary small">{tr('revokeHint')} {tr('admRevokeHint')}</p>
  {#if revokeError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[revokeError]}</p></div></div>{/if}
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={saving} onclick={() => (revokeOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="button" data-variant="danger" disabled={saving} aria-busy={saving} onclick={revoke}>
      {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{tr('admRevokeKey')}
    </button>
  {/snippet}
</Dialog>

<style>
  .key-name {
    font-weight: var(--weight-medium);
  }
  .scopes {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .small {
    font-size: var(--text-sm);
  }
  .mono.small {
    font-size: var(--text-xs);
  }
  .nowrap {
    white-space: nowrap;
  }
  .warn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--warning);
  }
  .warn :global(svg) {
    width: 13px;
    height: 13px;
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  .expires {
    display: grid;
    grid-template-columns: minmax(0, 240px) minmax(0, 1fr);
    gap: var(--space-3);
    align-items: end;
  }
  .secret {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-5);
  }
  .secret .input {
    flex: 1;
    min-width: 0;
  }
  .code {
    margin: 0;
    padding: 12px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.7;
    overflow-x: auto;
  }
  @media (max-width: 640px) {
    .expires {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
