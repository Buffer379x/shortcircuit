<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Api, DestinationPolicy, DestinationPolicyReport, DestinationRule } from './api';
  import { problemMessage } from './format';
  import Icon from './Icon.svelte';
  import type { Language, Messages } from './i18n';

  let { t, api, language }: { t: Messages; api: Api; language: Language } = $props();

  const headingId = $props.id();
  const count = $derived(new Intl.NumberFormat(language));
  let loading = $state(true);
  let busy = $state(false);
  let loadError = $state<keyof Messages | null>(null);
  let formError = $state<keyof Messages | null>(null);
  let notice = $state<keyof Messages | null>(null);
  let enabled = $state(false);
  let rules = $state<DestinationRule[]>([]);
  let report = $state<DestinationPolicyReport | null>(null);
  let reportIsDraft = $state(false);
  let newHost = $state('');
  let newSubdomains = $state(false);
  let newAction = $state<DestinationRule['action']>('allow');

  onMount(() => { void load(); });

  function apply(result: DestinationPolicyReport, draft: boolean) {
    report = result;
    reportIsDraft = draft;
    if (!draft) {
      enabled = result.enabled;
      rules = result.rules.map((rule) => ({ ...rule }));
    }
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      apply(await api<DestinationPolicyReport>('/destination-policy'), false);
    } catch (error) {
      loadError = problemMessage(error);
    } finally {
      loading = false;
    }
  }

  function draft(): DestinationPolicy {
    return { enabled, rules: rules.map((rule) => ({ ...rule, host: rule.host.trim() })) };
  }

  function addRule(event: SubmitEvent) {
    event.preventDefault();
    const host = newHost.trim();
    if (!host) return;
    rules = [...rules, { host, subdomains: newSubdomains, action: newAction }];
    newHost = '';
    newSubdomains = false;
    notice = null;
  }

  let ruleList = $state<HTMLElement>();
  let addHost = $state<HTMLInputElement>();
  let invalidRule = $state<number | null>(null);

  async function removeRule(index: number) {
    rules = rules.filter((_, position) => position !== index);
    notice = null;
    invalidRule = null;
    // Continue with the rule that took this place, or with the new-rule field.
    await tick();
    const buttons = ruleList?.querySelectorAll<HTMLButtonElement>('.delete-button') ?? [];
    (buttons[Math.min(index, buttons.length - 1)] ?? addHost)?.focus();
  }

  // Matches the server's syntax checks: a hostname or an IP address, without scheme or path.
  function plausibleHost(value: string) {
    const host = value.trim();
    if (/^\[[0-9a-fA-F:.]+\]$/.test(host)) return true;
    return host.length > 0 && host.length <= 255 && !/[\s/:@*?#]/.test(host)
      && !host.replace(/\.$/, '').split('.').some((label) => label.length === 0);
  }

  async function submit(path: '/destination-policy' | '/destination-policy/preview') {
    if (busy) return;
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const invalid = rules.findIndex((rule) => !plausibleHost(rule.host));
    if (invalid >= 0) {
      invalidRule = invalid;
      formError = 'invalidDestinationRule';
      await tick();
      ruleList?.querySelectorAll<HTMLInputElement>('input[name="rule-host"]')[invalid]?.focus();
      return;
    }
    invalidRule = null;
    busy = true;
    formError = null;
    notice = null;
    try {
      const preview = path.endsWith('/preview');
      const result = await api<DestinationPolicyReport>(path, preview ? 'POST' : 'PATCH', draft());
      apply(result, preview);
      if (!preview) notice = 'destinationPolicySaved';
    } catch (error) {
      formError = problemMessage(error);
    } finally {
      busy = false;
    }
    // The button was disabled while busy, which moves focus away. Return it.
    await tick();
    trigger?.focus();
  }
</script>

<section class="panel panel-pad stack" aria-labelledby={headingId} aria-busy={loading || busy}>
  <div class="stack compact">
    <h2 id={headingId}>{t.destinationPolicy}</h2>
    <p>{t.destinationPolicyDescription}</p>
    <p class="muted">{t.destinationPolicyPrecedence}</p>
  </div>
  {#if loadError}
    <p class="notice error" role="alert">{t[loadError]}</p>
    <div><button class="button" type="button" onclick={load}>{t.retry}</button></div>
  {:else if loading}
    <p role="status">{t.loading}</p>
  {:else}
    <div class="stack compact">
      <label class="row"><input type="checkbox" bind:checked={enabled} disabled={busy} aria-describedby={headingId + '-effect'} />{t.destinationPolicyEnforce}</label>
      <small id={headingId + '-effect'}>{t.destinationPolicyEffect}</small>
    </div>

    {#if rules.length}
      <ul class="rule-list" aria-label={t.destinationRules} bind:this={ruleList}>
        {#each rules as rule, index (index)}
          <li class="rule-row">
            <label class="field host-field"><span>{t.destinationHost}</span><input name="rule-host" bind:value={rule.host} required disabled={busy} autocomplete="off" spellcheck="false"
              aria-invalid={invalidRule === index} aria-describedby={invalidRule === index ? headingId + '-rule-error' : undefined} /></label>
            {#if invalidRule === index}<p class="notice error rule-error" id={headingId + '-rule-error'}>{t.invalidDestinationRule}</p>{/if}
            <label class="field"><span>{t.destinationRuleAction}</span>
              <select bind:value={rule.action} disabled={busy}>
                <option value="allow">{t.destinationAllow}</option>
                <option value="block">{t.destinationBlock}</option>
              </select>
            </label>
            <label class="row subdomains"><input type="checkbox" bind:checked={rule.subdomains} disabled={busy} />{t.destinationSubdomains}</label>
            <button class="icon-button ghost delete-button" type="button" aria-label={`${t.destinationRemoveRule}: ${rule.host}`} title={t.destinationRemoveRule} disabled={busy} onclick={() => removeRule(index)}><Icon name="trash" /></button>
          </li>
        {/each}
      </ul>
    {:else}
      <p>{t.destinationNoRules}</p>
    {/if}

    <form class="rule-row add-rule" onsubmit={addRule} aria-label={t.destinationAddRule}>
      <label class="field host-field"><span>{t.destinationHost}</span><input bind:this={addHost} bind:value={newHost} required disabled={busy} placeholder="example.com" autocomplete="off" spellcheck="false" /></label>
      <label class="field"><span>{t.destinationRuleAction}</span>
        <select bind:value={newAction} disabled={busy}>
          <option value="allow">{t.destinationAllow}</option>
          <option value="block">{t.destinationBlock}</option>
        </select>
      </label>
      <label class="row subdomains"><input type="checkbox" bind:checked={newSubdomains} disabled={busy} />{t.destinationSubdomains}</label>
      <button class="button" type="submit" disabled={busy}><Icon name="plus" />{t.destinationAddRule}</button>
    </form>
    <small>{t.destinationHostHint}</small>

    {#if formError}<p class="notice error" role="alert">{t[formError]}</p>{/if}
    {#if notice}<p class="notice" role="status">{t[notice]}</p>{/if}

    {#if report}
      <div class="stack compact" role="status">
        <h3>{reportIsDraft ? t.destinationImpactDraft : t.destinationImpactSaved}</h3>
        {#if !report.enabled}
          <p>{t.destinationImpactOff}</p>
        {:else}
          <dl class="report-notes">
            <div><dt>{t.destinationImpactLinks}</dt><dd>{count.format(report.impact.links)}</dd></div>
            <div><dt>{t.destinationImpactRules}</dt><dd>{count.format(report.impact.rule_links)}</dd></div>
            <div><dt>{t.destinationImpactFallbacks}</dt><dd>{count.format(report.impact.fallbacks)}</dd></div>
          </dl>
          {#if report.impact.examples.length}
            <p>{t.destinationImpactExamples}</p>
            <ul class="mono examples">
              {#each report.impact.examples as link (link.id)}<li>{link.host}/{link.slug}</li>{/each}
            </ul>
          {/if}
        {/if}
      </div>
    {/if}

    <div class="row actions">
      <button class="button" type="button" disabled={busy} onclick={() => submit('/destination-policy/preview')}>{t.destinationCheckImpact}</button>
      <button class="button primary" type="button" disabled={busy} onclick={() => submit('/destination-policy')}>{busy ? t.saving : t.destinationSave}</button>
    </div>
  {/if}
</section>

<style>
  section { margin-top: 16px; }
  .rule-list { display: grid; gap: 12px; margin: 0; padding: 0; list-style: none; }
  .rule-row { display: grid; grid-template-columns: minmax(12rem, 2fr) minmax(8rem, 1fr) auto auto; align-items: end; gap: 12px; }
  .rule-row + .rule-row { padding-block-start: 12px; border-block-start: 1px solid var(--line); }
  .subdomains { min-height: var(--hit); }
  .rule-error { grid-column: 1 / -1; margin: 0; }
  .examples { margin: 0; padding-inline-start: 1.25rem; overflow-wrap: anywhere; }
  .actions { flex-wrap: wrap; }
  @media (max-width: 720px) {
    .rule-row { grid-template-columns: 1fr; align-items: stretch; }
  }
</style>
