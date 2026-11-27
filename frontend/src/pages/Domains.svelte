<script lang="ts">
  // Public domains (origin, root and missing-slug fallbacks, crawler setting, enable state) and the
  // destination policy with its impact preview. The tab lives in the URL.
  import { onMount, tick } from 'svelte';
  import type { Domain, DomainDeletion, DomainDeletionPreview, DestinationPolicy, DestinationPolicyReport, DestinationRule } from '../api';
  import { problemMessage } from '../format';
  import type { Messages } from '../i18n';
  import Icon from '../lib/Icon.svelte';
  import PageHeader from '../lib/ui/PageHeader.svelte';
  import Dialog from '../lib/ui/Dialog.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import { app } from '../lib/app.svelte';
  import { num, splitUrl } from '../lib/fmt';
  import { locale, tr } from '../lib/locale.svelte';
  import { route, href, setParams } from '../lib/router.svelte';
  import { toast } from '../lib/toast.svelte';
  import { tip } from '../lib/tooltip';

  let { onchange }: { onchange?: () => void } = $props();

  type Tab = 'domains' | 'policy';
  const TABS: Tab[] = ['domains', 'policy'];
  let tab = $state<Tab>(route.params.get('tab') === 'policy' ? 'policy' : 'domains');
  $effect(() => setParams({ tab: tab === 'domains' ? null : tab }));

  function tabKeys(event: KeyboardEvent) {
    const index = TABS.indexOf(tab);
    const next = event.key === 'ArrowRight' ? index + 1 : event.key === 'ArrowLeft' ? index - 1 : event.key === 'Home' ? 0 : event.key === 'End' ? TABS.length - 1 : null;
    if (next === null) return;
    event.preventDefault();
    tab = TABS[(next + TABS.length) % TABS.length];
    void tick().then(() => document.getElementById(`dm-tab-${tab}`)?.focus());
  }

  // ---------- Domains ----------
  type DomainField = 'host' | 'origin' | 'root_target' | 'missing_target';
  let domains = $state<Domain[]>([]);
  let loading = $state(true);
  let loadError = $state<keyof Messages | null>(null);
  let dialogOpen = $state(false);
  let existing = $state(false);
  let draft = $state<Domain>({ host: '', origin: '', enabled: true, root_target: null, missing_target: null, block_crawlers: false });
  let saving = $state(false);
  let fieldError = $state<{ field: DomainField; message: keyof Messages } | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let formErrorBox = $state<HTMLDivElement>();

  onMount(() => {
    void load();
    void loadPolicy();
  });

  async function load() {
    loading = true;
    loadError = null;
    try {
      domains = (await app.api<{ items: Domain[] }>('/catalog/domains')).items;
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  function edit(item?: Domain) {
    existing = Boolean(item);
    draft = item ? { ...item } : { host: '', origin: '', enabled: true, root_target: null, missing_target: null, block_crawlers: false };
    fieldError = null;
    formError = null;
    dialogOpen = true;
  }

  function webUrl(value: string) {
    try {
      const url = new URL(value);
      return url.protocol === 'https:' || url.protocol === 'http:';
    } catch {
      return false;
    }
  }

  function errorFor(field: DomainField) {
    return fieldError?.field === field ? fieldError.message : null;
  }

  async function fail(field: DomainField, message: keyof Messages) {
    fieldError = { field, message };
    await tick();
    document.getElementById(`dm-${field}`)?.focus();
  }

  function suggestOrigin() {
    const host = draft.host.trim();
    if (!existing && host && !draft.origin.trim() && !/[\s/:]/.test(host)) draft.origin = `https://${host.toLowerCase()}`;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    fieldError = null;
    formError = null;
    const host = draft.host.trim();
    const origin = draft.origin.trim();
    const root = draft.root_target?.trim() || null;
    const missing = draft.missing_target?.trim() || null;
    if (!host) return fail('host', 'admHostRequired');
    if (!/^\[[0-9a-fA-F:.]+\]$/.test(host) && /[\s/:?#@]/.test(host)) return fail('host', 'admInvalidDomainHost');
    if (!existing && domains.some((item) => item.host === host.toLowerCase())) return fail('host', 'admDomainExists');
    if (!origin) return fail('origin', 'admOriginRequired');
    if (!webUrl(origin)) return fail('origin', 'invalidOrigin');
    if (root && !webUrl(root)) return fail('root_target', 'invalidFallback');
    if (missing && !webUrl(missing)) return fail('missing_target', 'invalidFallback');
    saving = true;
    let message: keyof Messages | null = null;
    try {
      const updated = await app.api<Domain>('/catalog/domains', 'POST', { ...draft, host, origin, root_target: root, missing_target: missing });
      domains = domains.some((item) => item.host === updated.host)
        ? domains.map((item) => (item.host === updated.host ? updated : item))
        : [...domains, updated];
      dialogOpen = false;
      toast(existing ? tr('admDomainSaved') : tr('admDomainAdded'), { body: updated.host });
      onchange?.();
    } catch (error) {
      message = problemMessage(error);
    } finally {
      saving = false;
    }
    if (!message) return;
    if (message === 'invalidHost') await fail('host', 'admInvalidDomainHost');
    else if (message === 'invalidOrigin') await fail('origin', message);
    else if (message === 'fallbackLoop') await fail('root_target', message);
    else {
      formError = message;
      await tick();
      formErrorBox?.focus();
    }
  }

  function described(field: DomainField, hint?: string) {
    return [errorFor(field) ? `dm-${field}-error` : '', hint ?? ''].filter(Boolean).join(' ') || undefined;
  }

  // ---------- Delete a domain ----------
  // The preview reports the link count, the move targets, and, for a chosen target, the slug
  // conflicts. The delete sends the previewed count, so a change in between returns 409.
  type LinkChoice = 'move' | 'delete';
  let removing = $state<Domain | null>(null);
  let removal = $state.raw<DomainDeletionPreview | null>(null);
  let removalLoading = $state(false);
  let removalBusy = $state(false);
  let removalError = $state<keyof Messages | null>(null);
  let removalErrorBox = $state<HTMLDivElement>();
  let linkChoice = $state<LinkChoice>('move');
  let moveTarget = $state('');
  let conflicts = $state.raw<DomainDeletionPreview['move'] | null>(null);
  let conflictsLoading = $state(false);
  let typedHost = $state('');
  let previewRequest = 0;

  const removalLinks = $derived(removal?.links ?? 0);
  const keysBlock = $derived(!!removal && removal.api_keys > 0 && !removal.environment);
  const canRemove = $derived.by(() => {
    if (!removal || removalLoading || removalBusy || keysBlock) return false;
    if (!removalLinks) return true;
    if (linkChoice === 'delete') return typedHost.trim().toLowerCase() === removal.host;
    return !!moveTarget && !conflictsLoading && conflicts?.target === moveTarget && conflicts.conflicts === 0;
  });

  function startRemoval(item: Domain) {
    removing = item;
    removal = null;
    removalError = null;
    linkChoice = 'move';
    moveTarget = '';
    conflicts = null;
    typedHost = '';
    void loadRemoval();
  }

  async function loadRemoval(target?: string) {
    if (!removing) return;
    const request = ++previewRequest;
    const params = new URLSearchParams({ host: removing.host });
    if (target) params.set('target', target);
    if (target) conflictsLoading = true;
    else removalLoading = true;
    try {
      const result = await app.api<DomainDeletionPreview>(`/catalog/domains/deletion?${params}`);
      if (request !== previewRequest) return;
      removal = result;
      conflicts = result.move ?? null;
      if (!result.targets.length) linkChoice = 'delete';
    } catch (error) {
      if (request !== previewRequest) return;
      removalError = problemMessage(error);
    } finally {
      if (request === previewRequest) {
        removalLoading = false;
        conflictsLoading = false;
      }
    }
  }

  function chooseTarget() {
    conflicts = null;
    removalError = null;
    if (moveTarget) void loadRemoval(moveTarget);
  }

  function nCount(n: number, one: keyof Messages, many: keyof Messages, values: Record<string, string> = {}) {
    return n === 1 ? tr(one, values) : tr(many, { ...values, count: num(n) });
  }

  async function confirmRemoval(event: SubmitEvent) {
    event.preventDefault();
    if (!removing || !removal || !canRemove) return;
    const host = removal.host;
    const links = removalLinks ? linkChoice : 'none';
    removalBusy = true;
    removalError = null;
    let failed: keyof Messages | null = null;
    try {
      const result = await app.api<DomainDeletion>('/catalog/domains', 'DELETE', {
        host,
        links,
        expected_links: removalLinks,
        ...(links === 'move' ? { target: moveTarget } : {}),
        ...(links === 'delete' ? { confirm: typedHost.trim() } : {}),
      });
      const summary = result.links === 'move'
        ? nCount(result.moved, 'domDeletedMovedOne', 'domDeletedMoved', { host, target: result.target ?? '' })
        : result.links === 'delete'
          ? nCount(result.deleted, 'domDeletedLinksOne', 'domDeletedLinks', { host })
          : host;
      toast(tr('domDeleted'), { body: result.environment ? `${summary}. ${tr('domDeletedServed', { host })}` : summary });
      removing = null;
      // An environment host stays in the list with default settings.
      if (result.environment) await load();
      else domains = domains.filter((item) => item.host !== host);
      onchange?.();
      await tick();
      document.querySelector<HTMLElement>('main h1')?.focus();
    } catch (error) {
      failed = problemMessage(error);
    } finally {
      removalBusy = false;
    }
    if (!failed) return;
    removalError = failed;
    // A changed count or new conflicts: show the current state next to the error.
    if (failed === 'domChanged' || failed === 'domMoveConflict' || failed === 'domHasKeys' || failed === 'domInvalidTarget') {
      const keep = removalError;
      if (failed === 'domInvalidTarget') moveTarget = '';
      await loadRemoval(linkChoice === 'move' && moveTarget ? moveTarget : undefined);
      removalError = keep;
      typedHost = '';
    }
    await tick();
    removalErrorBox?.focus();
  }

  // ---------- Destination policy ----------
  let policyLoading = $state(true);
  let policyError = $state<keyof Messages | null>(null);
  let policyBusy = $state<'preview' | 'save' | null>(null);
  let policyFormError = $state<keyof Messages | null>(null);
  let enabled = $state(false);
  let rules = $state<DestinationRule[]>([]);
  let saved = $state('');
  let report = $state.raw<DestinationPolicyReport | null>(null);
  let reportIsDraft = $state(false);
  let invalidRule = $state<number | null>(null);
  let ruleList = $state<HTMLElement>();
  let addButton = $state<HTMLButtonElement>();
  let testUrl = $state('');
  const policyDirty = $derived(JSON.stringify(policyDraft()) !== saved);

  function policyDraft(): DestinationPolicy {
    return { enabled, rules: rules.map((rule) => ({ ...rule, host: rule.host.trim() })) };
  }

  function applyReport(result: DestinationPolicyReport, draftReport: boolean) {
    report = result;
    reportIsDraft = draftReport;
    if (!draftReport) {
      enabled = result.enabled;
      rules = result.rules.map((rule) => ({ ...rule }));
      saved = JSON.stringify(policyDraft());
    }
  }

  async function loadPolicy() {
    policyLoading = true;
    policyError = null;
    try {
      applyReport(await app.api<DestinationPolicyReport>('/destination-policy'), false);
    } catch (error) {
      policyError = problemMessage(error);
    } finally {
      policyLoading = false;
    }
  }

  async function addRule() {
    rules = [...rules, { host: '', subdomains: true, action: 'block' }];
    await tick();
    const inputs = ruleList?.querySelectorAll<HTMLInputElement>('input[name="rule-host"]');
    inputs?.[inputs.length - 1]?.focus();
  }

  async function removeRule(index: number) {
    rules = rules.filter((_, position) => position !== index);
    invalidRule = null;
    policyFormError = null;
    // Continue with the rule that took this place, or with Add rule.
    await tick();
    const buttons = ruleList?.querySelectorAll<HTMLButtonElement>('.remove-rule') ?? [];
    (buttons[Math.min(index, buttons.length - 1)] ?? addButton)?.focus();
  }

  // Matches the server's syntax checks: a hostname or an IP address, without scheme or path.
  function plausibleHost(value: string) {
    const host = value.trim();
    if (/^\[[0-9a-fA-F:.]+\]$/.test(host)) return true;
    return host.length > 0 && host.length <= 255 && !/[\s/:@*?#]/.test(host)
      && !host.replace(/\.$/, '').split('.').some((label) => label.length === 0);
  }

  async function submitPolicy(preview: boolean) {
    if (policyBusy) return;
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const invalid = rules.findIndex((rule) => !plausibleHost(rule.host));
    if (invalid >= 0) {
      invalidRule = invalid;
      policyFormError = null;
      await tick();
      ruleList?.querySelectorAll<HTMLInputElement>('input[name="rule-host"]')[invalid]?.focus();
      return;
    }
    invalidRule = null;
    policyBusy = preview ? 'preview' : 'save';
    policyFormError = null;
    try {
      const result = await app.api<DestinationPolicyReport>(preview ? '/destination-policy/preview' : '/destination-policy', preview ? 'POST' : 'PATCH', policyDraft());
      applyReport(result, preview);
      if (!preview) toast(tr('destinationPolicySaved'), { body: tr('admPolicySummary', { count: result.rules.length, state: result.enabled ? tr('admEnforced') : tr('admNotEnforced') }) });
    } catch (error) {
      policyFormError = problemMessage(error);
    } finally {
      policyBusy = null;
    }
    // The button was disabled while busy, which moves focus away. Return it.
    await tick();
    trigger?.focus();
  }

  function canonical(host: string) {
    const value = host.trim().toLowerCase();
    try {
      return new URL(`http://${value}`).hostname.replace(/\.$/, '');
    } catch {
      return value.replace(/\.$/, '');
    }
  }

  // Applies the draft rules to one URL with the server's precedence: block first, then allow rules.
  const testResult = $derived.by(() => {
    const value = testUrl.trim();
    if (!value) return null;
    let host: string;
    try {
      const url = new URL(value);
      if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error();
      host = url.hostname.replace(/\.$/, '').toLowerCase();
    } catch {
      return { ok: false, text: tr('admTestInvalid') };
    }
    if (!enabled) return { ok: true, text: tr('admTestOff') };
    const matches = (rule: DestinationRule) => {
      const ruleHost = canonical(rule.host);
      return !!ruleHost && (host === ruleHost || (rule.subdomains && host.endsWith('.' + ruleHost)));
    };
    const block = rules.find((rule) => rule.action === 'block' && matches(rule));
    if (block) return { ok: false, text: tr('admTestBlocked', { host: block.host.trim() }) };
    const allows = rules.filter((rule) => rule.action === 'allow');
    if (!allows.length) return { ok: true, text: tr('admTestAllowed') };
    const allow = allows.find(matches);
    return allow ? { ok: true, text: tr('admTestAllowedBy', { host: allow.host.trim() }) } : { ok: false, text: tr('admTestNoAllow') };
  });

  const disabledState = $derived({ tone: 'muted' as const, icon: 'circle-pause' as const, label: tr('disabled') });
</script>

<div class="page">
  <PageHeader title={tr('navDomains')} description={tr('admDomainsDescription')}>
    {#snippet actions()}
      {#if tab === 'domains'}
        <button class="btn" data-variant="primary" disabled={loading || !!loadError} onclick={() => edit()}><Icon name="plus" />{tr('addDomain')}</button>
      {/if}
    {/snippet}
  </PageHeader>

  <div class="tabs" role="tablist" aria-label={tr('admDomainSections')} tabindex="-1" onkeydown={tabKeys}>
    <button id="dm-tab-domains" class="tab" role="tab" aria-selected={tab === 'domains'} aria-controls="dm-panel" tabindex={tab === 'domains' ? 0 : -1} onclick={() => (tab = 'domains')}>
      <Icon name="globe" />{tr('navDomains')}{#if !loading && !loadError}<span class="count">{domains.length}</span>{/if}
    </button>
    <button id="dm-tab-policy" class="tab" role="tab" aria-selected={tab === 'policy'} aria-controls="dm-panel" tabindex={tab === 'policy' ? 0 : -1} onclick={() => (tab = 'policy')}>
      <Icon name="shield" />{tr('destinationPolicy')}
    </button>
  </div>

  <div id="dm-panel" class="stack" role="tabpanel" aria-labelledby="dm-tab-{tab}">
    {#if tab === 'domains'}
      <section class="panel" aria-label={tr('navDomains')} aria-busy={loading}>
        {#if loadError}
          <div class="panel-body">
            <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[loadError]}</p></div><button class="btn" data-size="sm" onclick={load}>{tr('tryAgain')}</button></div>
          </div>
        {:else if loading}
          <p class="panel-body muted" role="status">{tr('loadingPage')}</p>
        {:else if !domains.length}
          <Empty icon="globe" title={tr('admNoDomains')}>{tr('admNoDomainsHint')}</Empty>
        {:else}
          <div class="table-wrap">
            <table class="table">
              <thead>
                <tr>
                  <th scope="col">{tr('domain')}</th>
                  <th scope="col">{tr('admRootAddress')}</th>
                  <th scope="col">{tr('admMissingSlug')}</th>
                  <th scope="col">{tr('admSearchEngines')}</th>
                  <th scope="col" class="c-actions"><span class="visually-hidden">{tr('actions')}</span></th>
                </tr>
              </thead>
              <tbody>
                {#each domains as item (item.host)}
                  <tr>
                    <td>
                      <div class="dom">
                        <span class="mono">{item.host}</span>
                        {#if !item.enabled}<Badge state={disabledState} />{/if}
                      </div>
                      {#if item.origin !== `https://${item.host}`}<span class="origin mono muted">{item.origin}</span>{/if}
                    </td>
                    <td>{@render target(item.root_target)}</td>
                    <td>{@render target(item.missing_target)}</td>
                    <td>{item.block_crawlers ? tr('admDiscouraged') : tr('admAllowed')}</td>
                    <td class="c-actions">
                      <div class="row-actions">
                        <button class="btn reveal" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('admEditDomainNamed', { host: item.host })} use:tip={tr('editDomain')} onclick={() => edit(item)}><Icon name="pencil" /></button>
                        <Menu label={tr('moreActionsFor', { name: item.host })} icon="ellipsis" size="sm">
                          {#snippet children(close)}
                            <button class="menu-item" role="menuitem" onclick={() => { close(); edit(item); }}><Icon name="pencil" />{tr('editDomain')}</button>
                            <a class="menu-item" role="menuitem" href={href('links', '', { host: item.host })} onclick={close}><Icon name="link" />{tr('admShowLinks')}</a>
                            <button class="menu-item" role="menuitem" data-tone="danger" onclick={() => { close(); startRemoval(item); }}><Icon name="trash-2" />{tr('domDeleteDomain')}</button>
                          {/snippet}
                        </Menu>
                      </div>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>
      <div class="notice" data-tone="neutral">
        <Icon name="info" />
        <div><b>{tr('admProxyTitle')}</b><p>{tr('admProxyText')}</p></div>
      </div>
    {:else}
      {#if policyError}
        <div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[policyError]}</p></div><button class="btn" data-size="sm" onclick={loadPolicy}>{tr('tryAgain')}</button></div>
      {:else if policyLoading}
        <p class="muted" role="status">{tr('loadingPage')}</p>
      {:else}
        <div class="grid-main-side">
          <section class="panel" aria-labelledby="dm-rules-title" aria-busy={!!policyBusy}>
            <header class="panel-head">
              <div class="panel-title"><h2 id="dm-rules-title">{tr('admRules')}</h2><p>{tr('admRulesHint')}</p></div>
              <span class="spacer"></span>
              <label class="check enforce">
                <input class="switch" type="checkbox" bind:checked={enabled} disabled={!!policyBusy} aria-label={tr('destinationPolicyEnforce')} aria-describedby="dm-effect" />
                <span aria-hidden="true">{enabled ? tr('admEnforcedLabel') : tr('admOffLabel')}</span>
              </label>
            </header>
            <div class="panel-body policy-body">
              {#if rules.length}
                <ul class="rules" class:off={!enabled} aria-label={tr('destinationRules')} bind:this={ruleList}>
                  {#each rules as rule, index (index)}
                    <li>
                      <div class="rule">
                        <select class="select action" data-size="sm" aria-label={tr('admRuleAction', { n: index + 1 })} bind:value={rule.action} disabled={!!policyBusy}>
                          <option value="block">{tr('destinationBlock')}</option>
                          <option value="allow">{tr('destinationAllow')}</option>
                        </select>
                        <input
                          class="input mono"
                          data-size="sm"
                          name="rule-host"
                          aria-label={tr('admRuleHost', { n: index + 1 })}
                          placeholder="example.com"
                          autocomplete="off"
                          spellcheck="false"
                          bind:value={rule.host}
                          disabled={!!policyBusy}
                          aria-invalid={invalidRule === index ? 'true' : undefined}
                          aria-describedby={invalidRule === index ? 'dm-rule-error' : undefined}
                        />
                        <label class="check sub"><input class="checkbox" type="checkbox" bind:checked={rule.subdomains} disabled={!!policyBusy} /><span>{tr('admSubdomains')}</span></label>
                        <button class="btn remove-rule" type="button" data-variant="ghost" data-size="sm" data-icon-only aria-label={tr('admRemoveRule', { n: index + 1, host: rule.host || '…' })} use:tip={tr('destinationRemoveRule')} disabled={!!policyBusy} onclick={() => removeRule(index)}><Icon name="trash-2" /></button>
                      </div>
                      {#if invalidRule === index}<p class="field-error" id="dm-rule-error"><Icon name="circle-alert" />{tr('invalidDestinationRule')}</p>{/if}
                    </li>
                  {/each}
                </ul>
              {:else}
                <p class="muted empty-rules">{tr('destinationNoRules')}</p>
              {/if}
              <p class="field-hint">{tr('destinationHostHint')} {tr('destinationPolicyPrecedence')}</p>
              <p class="field-hint" id="dm-effect">{tr('destinationPolicyEffect')}</p>
              {#if policyFormError}<div class="notice" data-tone="error" role="alert"><Icon name="circle-alert" /><div><p>{locale.t[policyFormError]}</p></div></div>{/if}
            </div>
            <footer class="panel-foot">
              <button class="btn" type="button" bind:this={addButton} disabled={!!policyBusy} onclick={addRule}><Icon name="plus" />{tr('destinationAddRule')}</button>
              <span class="spacer"></span>
              {#if policyDirty}<span class="unsaved muted"><Icon name="circle-dot" />{tr('admUnsaved')}</span>{/if}
              <button class="btn" data-variant="primary" type="button" disabled={!!policyBusy} aria-busy={policyBusy === 'save'} onclick={() => submitPolicy(false)}>
                {#if policyBusy === 'save'}<span class="spinner" aria-hidden="true"></span>{/if}{tr('destinationSave')}
              </button>
            </footer>
          </section>

          <div class="stack">
            <section class="panel" aria-labelledby="dm-test-title">
              <header class="panel-head"><div class="panel-title"><h2 id="dm-test-title">{tr('admTestTitle')}</h2><p>{tr('admTestHint')}</p></div></header>
              <div class="panel-body test">
                <input class="input mono" aria-label={tr('admTestLabel')} placeholder="https://docs.example.com/page" spellcheck="false" autocomplete="off" bind:value={testUrl} aria-describedby="dm-test-result" />
                <p class="result" id="dm-test-result" role="status" data-ok={testResult ? String(testResult.ok) : undefined}>
                  {#if testResult}<Icon name={testResult.ok ? 'circle-check' : 'ban'} />{testResult.text}{/if}
                </p>
              </div>
            </section>
            <section class="panel" aria-labelledby="dm-impact-title" aria-busy={policyBusy === 'preview'}>
              <header class="panel-head"><div class="panel-title"><h2 id="dm-impact-title">{reportIsDraft ? tr('destinationImpactDraft') : tr('destinationImpactSaved')}</h2></div></header>
              <div class="panel-body" role="status">
                {#if report && !report.enabled}
                  <p class="secondary small">{tr('destinationImpactOff')}</p>
                {:else if report}
                  <dl class="facts">
                    <dt>{tr('destinationImpactLinks')}</dt><dd class="num">{num(report.impact.links)}</dd>
                    <dt>{tr('destinationImpactRules')}</dt><dd class="num">{num(report.impact.rule_links)}</dd>
                    <dt>{tr('destinationImpactFallbacks')}</dt><dd class="num">{num(report.impact.fallbacks)}</dd>
                  </dl>
                  {#if report.impact.examples.length}
                    <p class="secondary small examples-title">{tr('destinationImpactExamples')}</p>
                    <ul class="examples">
                      {#each report.impact.examples as link (link.id)}<li><a class="mono" href={href('links', link.id)}>{link.host}/{link.slug}</a></li>{/each}
                    </ul>
                  {/if}
                {/if}
              </div>
              <footer class="panel-foot">
                <button class="btn" type="button" disabled={!!policyBusy} aria-busy={policyBusy === 'preview'} onclick={() => submitPolicy(true)}>
                  {#if policyBusy === 'preview'}<span class="spinner" aria-hidden="true"></span>{/if}{tr('destinationCheckImpact')}
                </button>
              </footer>
            </section>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>

{#snippet target(value: string | null)}
  {#if value}
    {@const parts = splitUrl(value)}
    <span class="dest cell-dest" use:tip={value}>{tr('admRedirectsTo')} <b>{parts.host}{parts.rest}</b></span>
  {:else}
    <span class="muted">{tr('admNotFoundPage')}</span>
  {/if}
{/snippet}

{#snippet fieldErr(field: DomainField)}
  {@const message = errorFor(field)}
  {#if message}<p class="field-error" id="dm-{field}-error"><Icon name="circle-alert" />{locale.t[message]}</p>{/if}
{/snippet}

<Dialog bind:open={dialogOpen} title={existing ? tr('editDomain') : tr('addDomain')} width={520} busy={saving}>
  <form id="dm-form" class="form" onsubmit={save} novalidate aria-busy={saving}>
    <div class="field">
      <label class="field-label" for="dm-host">{tr('admHost')}</label>
      <input id="dm-host" class="input mono" bind:value={draft.host} readonly={existing} disabled={saving} placeholder="go.example.com" autocomplete="off" autocapitalize="none" spellcheck="false" onblur={suggestOrigin} aria-invalid={errorFor('host') ? 'true' : undefined} aria-describedby={described('host', 'dm-host-hint')} />
      {@render fieldErr('host')}
      <p class="field-hint" id="dm-host-hint">{existing ? tr('admHostFixed') : tr('admHostHint')}</p>
    </div>
    <div class="field">
      <label class="field-label" for="dm-origin">{tr('publicOrigin')}</label>
      <input id="dm-origin" class="input mono" type="url" bind:value={draft.origin} disabled={saving} placeholder="https://go.example.com" autocomplete="off" spellcheck="false" aria-invalid={errorFor('origin') ? 'true' : undefined} aria-describedby={described('origin', 'dm-origin-hint')} />
      {@render fieldErr('origin')}
      <p class="field-hint" id="dm-origin-hint">{tr('admOriginHint')}</p>
    </div>
    <div class="field">
      <label class="field-label" for="dm-root_target">{tr('admRootAddress')} <span class="optional">{tr('admOptional')}</span></label>
      <input id="dm-root_target" class="input mono" type="url" bind:value={draft.root_target} disabled={saving} placeholder="https://example.com/" autocomplete="off" spellcheck="false" aria-invalid={errorFor('root_target') ? 'true' : undefined} aria-describedby={described('root_target', 'dm-root-hint')} />
      {@render fieldErr('root_target')}
      <p class="field-hint" id="dm-root-hint">{tr('admRootHint')}</p>
    </div>
    <div class="field">
      <label class="field-label" for="dm-missing_target">{tr('admMissingSlug')} <span class="optional">{tr('admOptional')}</span></label>
      <input id="dm-missing_target" class="input mono" type="url" bind:value={draft.missing_target} disabled={saving} placeholder="https://example.com/not-found" autocomplete="off" spellcheck="false" aria-invalid={errorFor('missing_target') ? 'true' : undefined} aria-describedby={described('missing_target', 'dm-missing-hint')} />
      {@render fieldErr('missing_target')}
      <p class="field-hint" id="dm-missing-hint">{tr('admMissingHint')}</p>
    </div>
    <label class="check"><input class="switch" type="checkbox" bind:checked={draft.enabled} disabled={saving} /><span>{tr('admDomainEnabled')}<small>{tr('admDomainEnabledHint')}</small></span></label>
    <label class="check">
      <input class="switch" type="checkbox" checked={!draft.block_crawlers} disabled={saving} onchange={(e) => (draft.block_crawlers = !e.currentTarget.checked)} />
      <span>{tr('admAllowSearchEngines')}<small>{tr('admAllowSearchEnginesHint')}</small></span>
    </label>
    {#if formError}<div class="notice" data-tone="error" role="alert" tabindex="-1" bind:this={formErrorBox}><Icon name="circle-alert" /><div><p>{locale.t[formError]}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={saving} onclick={() => (dialogOpen = false)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="dm-form" data-variant="primary" disabled={saving} aria-busy={saving}>
      {#if saving}<span class="spinner" aria-hidden="true"></span>{/if}{existing ? tr('admSaveDomain') : tr('addDomain')}
    </button>
  {/snippet}
</Dialog>

<Dialog open={removing !== null} onclose={() => (removing = null)} title={tr('domDeleteNamed', { host: removing?.host ?? '' })} width={520} busy={removalBusy}>
  <form id="dm-delete" class="form" onsubmit={confirmRemoval} novalidate aria-busy={removalBusy || removalLoading}>
    {#if removalLoading && !removal}
      <p class="muted" role="status">{tr('domChecking')}</p>
    {:else if removal}
      {@const host = removal.host}
      {#if removal.environment}
        <div class="notice" data-tone="neutral"><Icon name="info" /><div><b>{tr('domEnvironmentTitle')}</b><p>{tr('domEnvironmentText')}</p></div></div>
      {/if}
      {#if keysBlock}
        <div class="notice" data-tone="warning">
          <Icon name="key-round" />
          <div><p>{nCount(removal.api_keys, 'domOneKey', 'domKeys')}</p><p><a href={href('keys')}>{tr('domOpenKeys')}</a></p></div>
        </div>
      {/if}
      {#if !removalLinks}
        <p class="secondary confirm-text">{tr('domNoLinks')}</p>
      {:else}
        <p class="secondary confirm-text">{nCount(removalLinks, 'domOneLink', 'domLinks')}</p>
        <fieldset class="fieldset">
          <legend>{tr('domChoice')}</legend>
          <div class="choice-grid" role="radiogroup" aria-label={tr('domChoice')}>
            <label class="choice"><input class="radio" type="radio" name="dm-links" value="move" bind:group={linkChoice} disabled={removalBusy || !removal.targets.length} /><span><b>{tr('domMove')}</b><small>{tr('domMoveHint')}</small></span></label>
            <label class="choice"><input class="radio" type="radio" name="dm-links" value="delete" bind:group={linkChoice} disabled={removalBusy} /><span><b>{tr('domDeleteLinks')}</b><small>{tr('domDeleteLinksHint')}</small></span></label>
          </div>
        </fieldset>
        {#if linkChoice === 'move'}
          {#if !removal.targets.length}
            <p class="field-hint">{tr('domNoTargets')}</p>
          {:else}
            <div class="field">
              <label class="field-label" for="dm-target">{tr('domTarget')}</label>
              <select id="dm-target" class="select mono" value={moveTarget} onchange={(event) => { moveTarget = event.currentTarget.value; chooseTarget(); }} disabled={removalBusy} aria-describedby="dm-target-hint dm-conflicts">
                <option value="" disabled>{tr('domChooseTarget')}</option>
                {#each removal.targets as target (target)}<option value={target}>{target}</option>{/each}
              </select>
              <p class="field-hint" id="dm-target-hint">{tr('domTargetHint')}</p>
            </div>
            <div id="dm-conflicts" role="status">
              {#if moveTarget && conflictsLoading}
                <p class="muted confirm-text">{tr('domCheckingConflicts', { host: moveTarget })}</p>
              {:else if conflicts && conflicts.target === moveTarget && conflicts.conflicts === 0}
                <p class="secondary confirm-text"><Icon name="circle-check" />{tr('domNoConflicts', { host: conflicts.target })}</p>
              {:else if conflicts && conflicts.target === moveTarget}
                <div class="notice" data-tone="error">
                  <Icon name="circle-alert" />
                  <div>
                    <p>{nCount(conflicts.conflicts, 'domOneConflict', 'domConflicts', { host: conflicts.target })}</p>
                    <ul class="slug-list">
                      {#each conflicts.conflicting_slugs as slug (slug)}<li class="mono">{slug}</li>{/each}
                    </ul>
                    {#if conflicts.conflicts > conflicts.conflicting_slugs.length}<p>{tr('domMoreConflicts', { count: num(conflicts.conflicts - conflicts.conflicting_slugs.length) })}</p>{/if}
                  </div>
                  <button class="btn" type="button" data-size="sm" disabled={removalBusy} onclick={chooseTarget}>{tr('domCheckAgain')}</button>
                </div>
              {/if}
            </div>
            <div class="notice" data-tone="warning"><Icon name="triangle-alert" /><div><p>{tr('domMoveWarning', { host })}</p></div></div>
          {/if}
        {:else}
          <div class="notice" data-tone="warning">
            <Icon name="triangle-alert" />
            <div><p>{tr('domExportFirst')}</p><p><a href={href('exchange')}>{tr('domOpenExport')}</a></p></div>
          </div>
          <div class="field">
            <label class="field-label" for="dm-confirm">{tr('domTypeToConfirm', { host })}</label>
            <input id="dm-confirm" class="input mono" bind:value={typedHost} disabled={removalBusy} autocomplete="off" autocapitalize="none" spellcheck="false" />
          </div>
        {/if}
      {/if}
    {/if}
    {#if removalError}<div class="notice" data-tone="error" role="alert" tabindex="-1" bind:this={removalErrorBox}><Icon name="circle-alert" /><div><p>{locale.t[removalError]}</p></div></div>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" type="button" data-variant="ghost" disabled={removalBusy} onclick={() => (removing = null)}>{tr('cancel')}</button>
    <button class="btn" type="submit" form="dm-delete" data-variant="danger" disabled={!canRemove} aria-busy={removalBusy}>
      {#if removalBusy}<span class="spinner" aria-hidden="true"></span>{/if}<Icon name="trash-2" /><span>{tr('domDeleteDomain')}</span>
    </button>
  {/snippet}
</Dialog>

<style>
  .panel-foot > .spacer {
    flex: 1;
  }
  .dom {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .origin {
    display: block;
    margin-top: 2px;
    font-size: var(--text-xs);
  }
  .cell-dest {
    max-width: 260px;
  }
  .form {
    display: grid;
    gap: var(--space-5);
  }
  .confirm-text {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
  }
  .confirm-text :global(svg) {
    flex: none;
    width: 16px;
    height: 16px;
    color: var(--success);
  }
  .slug-list {
    max-height: min(30dvh, 12rem);
    overflow: auto;
    margin-top: var(--space-2);
    padding-left: var(--space-5);
    list-style: disc;
    overflow-wrap: anywhere;
  }
  .enforce {
    flex: none;
    align-items: center;
    font-size: var(--text-sm);
  }
  .policy-body {
    display: grid;
    gap: var(--space-3);
  }
  .rules {
    display: grid;
    gap: var(--space-2);
  }
  .rules.off {
    opacity: 0.55;
  }
  .rule {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rules li {
    display: grid;
    gap: 4px;
  }
  .action {
    flex: none;
    width: 104px;
  }
  .rule .input {
    flex: 1;
    min-width: 0;
  }
  .sub {
    flex: none;
    align-items: center;
    font-size: var(--text-sm);
  }
  .empty-rules,
  .small {
    font-size: var(--text-sm);
  }
  .unsaved {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-xs);
  }
  .unsaved :global(svg) {
    width: 12px;
    height: 12px;
    color: var(--warning);
  }
  .test {
    display: grid;
    gap: var(--space-3);
  }
  .result {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 20px;
    font-size: var(--text-sm);
    color: var(--error);
  }
  .result[data-ok='true'] {
    color: var(--success);
  }
  .result :global(svg) {
    flex: none;
    width: 16px;
    height: 16px;
  }
  .examples-title {
    margin-top: var(--space-4);
  }
  .examples {
    display: grid;
    gap: 4px;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }
  @media (max-width: 640px) {
    .rule {
      flex-wrap: wrap;
    }
    .rule .input {
      flex-basis: calc(100% - 112px);
    }
  }
</style>
