<script lang="ts">
  // New link: a dialog with the link form, then a success view with the short link, its QR code,
  // and the next steps. "Create another" keeps the domain and tags for a series of links.
  import { untrack } from 'svelte';
  import type { Link, LinkProtectionChange } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Icon from '../lib/Icon.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import LinkForm, {
    createSeed, emptyDraft, errorFor, monitoringEnabled, policyOf, readyContext, validateDraft,
    type FormContext, type FormErrors, type LinkDraft,
  } from './LinkForm.svelte';
  import { overlays, linksChanged } from '../lib/overlays.svelte';
  import { app, loadTags, shortUrl } from '../lib/app.svelte';
  import { go } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tr } from '../lib/locale.svelte';

  const defaultHost = () => app.domains?.hosts[0] ?? '';
  let draft = $state<LinkDraft>(emptyDraft(defaultHost()));
  let ctx = $state<FormContext>(readyContext());
  let errors = $state<FormErrors>({});
  let busy = $state(false);
  let created = $state<Link | null>(null);
  let followUp = $state<keyof Messages | null>(null);
  let another = $state(false);
  let duplicating = $state(false);
  let qrFailed = $state(false);
  let formKey = $state(0);

  $effect(() => {
    if (!overlays.create) return;
    untrack(() => {
      const seed = createSeed.link;
      createSeed.link = null;
      duplicating = seed != null;
      draft = seed
        ? {
            ...emptyDraft(app.domains?.hosts.includes(seed.host) ? seed.host : defaultHost()),
            target: seed.target, title: seed.title, description: seed.description, tags: [...seed.tags],
            analytics_enabled: seed.analytics_enabled,
          }
        : { ...emptyDraft(defaultHost()), target: overlays.createTarget };
      ctx = readyContext();
      errors = {};
      created = null;
      followUp = null;
      formKey++;
      void monitoringEnabled().then((on) => (ctx.monitoring = on));
    });
  });

  const policyChanged = (d: LinkDraft) =>
    d.redirect_status !== 302 || !!d.starts_at || !!d.expires_at || !!d.max_visits.trim() || d.forward_query || d.forward_path || d.noindex;

  async function submit(ev: SubmitEvent) {
    ev.preventDefault();
    if (busy) return;
    const found = validateDraft(draft, ctx);
    errors = found;
    if (Object.keys(found).length) return;
    busy = true;
    const d = $state.snapshot(draft);
    const protection: Omit<LinkProtectionChange, 'remove_password'> | undefined =
      d.access === 'none' ? undefined : { one_time: d.access === 'one_time', ...(d.password ? { password: d.password } : {}) };
    let link: Link;
    try {
      link = await app.api<Link>('/links', 'POST', {
        target: d.target.trim(),
        title: d.title.trim(),
        description: d.description.trim(),
        tags: d.tags,
        analytics_enabled: d.analytics_enabled,
        ...(duplicating ? { enabled: false } : {}),
        ...(d.host ? { host: d.host } : {}),
        ...(d.slug.trim() ? { slug: d.slug.trim() } : {}),
        ...(protection ? { protection } : {}),
      });
    } catch (error) {
      errors = errorFor(problemMessage(error));
      busy = false;
      return;
    }
    draft.password = '';
    // The link exists now. Redirect settings and monitoring are separate requests; a failure is
    // reported without hiding the created link.
    let later: keyof Messages | null = null;
    if (policyChanged(d)) {
      try {
        await app.api(`/links/${link.id}/policy`, 'PATCH', policyOf(d, []));
      } catch (error) {
        later = problemMessage(error);
      }
    }
    if (d.monitor && ctx.monitoring) {
      try {
        await app.api(`/monitoring/links/${link.id}`, 'PATCH', { enabled: true, priority: false });
      } catch (error) {
        later ??= problemMessage(error);
      }
    }
    busy = false;
    linksChanged();
    void loadTags();
    if (app.counts.links != null) app.counts.links++;
    if (another && !later) {
      toast(tr(duplicating ? 'duplicateCreated' : 'linkCreated'), { body: shortUrl(link) });
      restart(draft.host, draft.tags);
    } else {
      created = link;
      followUp = later;
      qrFailed = false;
      requestAnimationFrame(() => document.querySelector<HTMLElement>('.create-done .copy')?.focus());
    }
  }

  function restart(host: string, tags: string[] = []) {
    created = null;
    followUp = null;
    duplicating = false;
    draft = { ...emptyDraft(host), tags };
    errors = {};
    formKey++;
    requestAnimationFrame(() => document.getElementById('lf-target')?.focus());
  }

  function openDetails(tab?: string) {
    const id = created!.id;
    overlays.create = false;
    go('links', id, tab ? { tab } : undefined);
  }

  const title = $derived(created ? tr(duplicating ? 'lnkDuplicateCreatedTitle' : 'lnkCreatedTitle') : duplicating ? tr('duplicateLink') : tr('newLink'));
</script>

<Dialog bind:open={overlays.create} {title} description={created ? undefined : duplicating ? tr('duplicateHint') : tr('lnkCreateDescription')} width={600} {busy}>
  {#if created}
    <div class="done create-done">
      <div class="done-url">
        {#if created.enabled}
          <span class="badge" data-tone="success"><Icon name="circle-check" />{tr('lnkLive')}</span>
        {:else}
          <span class="badge" data-tone="muted"><Icon name="circle-pause" />{tr('stateDisabled')}</span>
        {/if}
        <p class="url"><span class="host">{shortUrl(created).slice(0, -created.slug.length)}</span><span class="slug">{created.slug}</span></p>
        <p class="dest truncate" title={created.target}>{created.target}</p>
        <div class="btn-group">
          <CopyButton value={shortUrl(created)} variant="primary" size="md" text={tr('copyShortLink')} />
          <a class="btn" href={shortUrl(created)} target="_blank" rel="noreferrer noopener"><Icon name="external-link" />{tr('lnkOpen')}</a>
          <button type="button" class="btn" onclick={() => (overlays.qrId = created!.id)}><Icon name="qr-code" />{tr('qrCode')}</button>
        </div>
      </div>
      {#if !qrFailed}
        <div class="done-qr">
          <img src="/api/v1/links/{created.id}/qr?format=svg&size=256" alt={tr('lnkQrFor', { url: shortUrl(created) })} width="112" height="112" onerror={() => (qrFailed = true)} />
        </div>
      {/if}
    </div>
    {#if followUp}
      <div class="notice" data-tone="warning" role="alert">
        <Icon name="triangle-alert" />
        <div><p><b>{tr('lnkFollowUpFailed')}</b></p><p>{tr(followUp)}</p></div>
      </div>
    {/if}
    <ul class="next">
      <li><button type="button" class="menu-item" onclick={() => openDetails('rules')}><Icon name="split" />{tr('lnkAddRules')}<span class="meta">{tr('lnkAddRulesMeta')}</span></button></li>
      <li><button type="button" class="menu-item" onclick={() => openDetails()}><Icon name="chart-column" />{tr('lnkOpenDetails')}<span class="meta">{tr('lnkOpenDetailsMeta')}</span></button></li>
    </ul>
  {:else}
    <form id="create-link" class="create-form" onsubmit={submit} novalidate aria-busy={busy}>
      {#key formKey}
        <LinkForm bind:draft {errors} mode="create" {ctx} {busy} />
      {/key}
    </form>
  {/if}
  {#snippet footer()}
    {#if created}
      <button type="button" class="btn" data-variant="ghost" onclick={() => restart(created?.host ?? defaultHost())}><Icon name="plus" />{tr('lnkCreateAnother')}</button>
      <span class="spacer"></span>
      <button type="button" class="btn" onclick={() => (overlays.create = false)}>{tr('lnkDone')}</button>
    {:else}
      <label class="check another"><input class="checkbox" type="checkbox" bind:checked={another} disabled={busy} />{tr('lnkCreateAnother')}</label>
      <span class="spacer"></span>
      <button class="btn" data-variant="ghost" type="button" onclick={() => (overlays.create = false)} disabled={busy}>{tr('cancel')}</button>
      <button class="btn" data-variant="primary" type="submit" form="create-link" aria-busy={busy} disabled={busy}>
        {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}<span>{duplicating ? tr('createDuplicate') : tr('addLink')}</span>
      </button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .create-form {
    display: grid;
    gap: var(--space-5);
  }
  .another {
    font-size: var(--text-sm);
  }
  .done {
    display: flex;
    gap: var(--space-5);
    align-items: center;
    padding: var(--space-5);
    border: 1px solid var(--accent-soft-border);
    border-radius: var(--radius-lg);
    background: var(--accent-soft);
  }
  .done-url {
    display: grid;
    gap: 8px;
    flex: 1;
    min-width: 0;
    justify-items: start;
  }
  .url {
    max-width: 100%;
    font-family: var(--font-mono);
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
    overflow-wrap: anywhere;
  }
  .url .host {
    color: var(--text-muted);
  }
  .url .slug {
    color: var(--text);
    font-weight: var(--weight-semibold);
  }
  .done .dest {
    max-width: 100%;
    margin-bottom: 6px;
  }
  .done-qr {
    padding: 8px;
    border-radius: var(--radius-md);
    background: var(--qr-bg);
  }
  .done-qr img {
    display: block;
    width: 112px;
    height: 112px;
  }
  .next {
    display: grid;
    gap: 2px;
  }
  .next .menu-item {
    width: 100%;
    min-height: 40px;
  }
  @media (max-width: 560px) {
    .done-qr {
      display: none;
    }
  }
</style>
