<script lang="ts">
  import { onMount } from 'svelte';
  import { linkName } from './api';
  import type { Api, Link, LinkProtection, LinkProtectionChange } from './api';
  import Dialog from './Dialog.svelte';
  import { passwordAllowed, problemMessage } from './format';
  import type { Messages } from './i18n';
  import ProtectionFields from './ProtectionFields.svelte';

  let { t, api, link, onclose, onsaved }: {
    t: Messages; api: Api; link: Link; onclose: () => void; onsaved: () => void;
  } = $props();

  const path = $derived(`/links/${link.id}/protection`);
  let protection = $state<LinkProtection | null>(null);
  let oneTime = $state(false);
  let password = $state('');
  let removePassword = $state(false);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<keyof Messages | null>(null);

  const dirty = $derived(oneTime !== protection?.one_time || !!password || removePassword);

  onMount(() => { void load(); });

  async function load() {
    if (loading) return;
    loading = true;
    error = null;
    try {
      const result = await api<LinkProtection>(path);
      oneTime = result.one_time;
      protection = result;
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving || !protection || protection.consumed || !dirty) return;
    error = null;
    if (password && !passwordAllowed(password)) {
      error = 'invalidPassword';
      return;
    }
    const update: LinkProtectionChange = {
      one_time: oneTime,
      ...(removePassword ? { remove_password: true } : password ? { password } : {}),
    };
    saving = true;
    try {
      await api<LinkProtection>(path, 'PATCH', update);
      password = '';
      onsaved();
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      saving = false;
    }
  }
</script>

<Dialog {t} title={t.linkProtection} open={true} busy={saving} {onclose}>
  <form onsubmit={save} aria-busy={loading || saving}>
    <div class="dialog-body stack">
      <strong>{linkName(link)}</strong>
      {#if loading}
        <p role="status">{t.loading}</p>
      {:else if protection?.consumed}
        <p role="status">{t.oneTimeConsumed}</p>
      {:else if protection}
        <ProtectionFields {t} bind:oneTime bind:password bind:removePassword
          hasPassword={protection.password_required} busy={saving} invalidPassword={error === 'invalidPassword'} />
      {/if}
      {#if error}
        <p class="notice error" role="alert">{t[error]}</p>
        {#if !protection}<div><button class="button" type="button" onclick={load}>{t.retry}</button></div>{/if}
      {/if}
    </div>
    {#if protection && !protection.consumed}
      <footer class="dialog-foot row">
        <button class="button" type="button" disabled={saving} onclick={onclose}>{t.cancel}</button>
        <button class="button primary" type="submit" disabled={saving || !dirty}>{saving ? t.saving : t.save}</button>
      </footer>
    {/if}
  </form>
</Dialog>
