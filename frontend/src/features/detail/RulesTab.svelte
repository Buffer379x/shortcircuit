<script lang="ts">
  // Ordered redirect rules with a request simulator. Saving re-reads the policy first and replaces only
  // the rules, so schedule, limit, and forwarding changes made elsewhere in the meantime are kept.
  import { tick, untrack } from 'svelte';
  import type { ConditionType, LinkPolicy, Policy, RedirectRule, SimulationResult } from '../../api';
  import type { Messages } from '../../i18n';
  import { problemMessage } from '../../format';
  import Icon from '../../lib/Icon.svelte';
  import { app } from '../../lib/app.svelte';
  import { splitUrl } from '../../lib/fmt';
  import { tr } from '../../lib/locale.svelte';
  import { toast } from '../../lib/toast.svelte';
  import { tip } from '../../lib/tooltip';
  import {
    MAX_CONDITIONS, MAX_RULES, cleanRules, conditionLabels, conditionOptions, newCondition, optionLabel,
    validateRules, type RuleErrors,
  } from './rules';

  let { linkId, target, policy, onsaved }: {
    linkId: number;
    /** The default destination of the link. */
    target: string;
    policy: Policy;
    onsaved: (policy: Policy) => void;
  } = $props();

  const uid = $props.id();
  const path = $derived(`/links/${linkId}/policy`);
  // Sample visitors fill the User-Agent field; the server classifies device, system, and browser from it.
  const agents = [
    ['detAgentIphone', 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1'],
    ['detAgentAndroid', 'Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36'],
    ['detAgentIpad', 'Mozilla/5.0 (iPad; CPU OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1'],
    ['detAgentWindows', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0'],
    ['detAgentMac', 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7_1) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15'],
    ['detAgentLinux', 'Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0'],
  ] as const satisfies readonly (readonly [keyof Messages, string])[];

  let rules = $state<RedirectRule[]>([]);
  let base = $state('');
  let source: Policy | null = null;
  let errors = $state<RuleErrors>({});
  let saving = $state(false);
  let saveError = $state<keyof Messages | null>(null);
  const dirty = $derived(JSON.stringify(cleanRules(rules)) !== base);

  let sampleQuery = $state('');
  let sampleLanguage = $state('');
  let sampleAgent = $state('');
  let samplePath = $state('');
  let simulating = $state(false);
  let simulationError = $state<keyof Messages | null>(null);
  let simulation = $state.raw<{ key: string; result: SimulationResult } | null>(null);
  const currentSimulation = $derived(simulation && simulation.key === JSON.stringify(simulationInput()) ? simulation.result : null);
  const fallback = $derived(splitUrl(target));

  // Adopt the saved rules when they change and the draft has no unsaved edits.
  $effect(() => {
    const next = policy;
    if (next === source) return;
    untrack(() => {
      const edited = source !== null && JSON.stringify(cleanRules(rules)) !== base;
      source = next;
      base = JSON.stringify(cleanRules(next.rules));
      if (edited) return;
      rules = cleanRules(next.rules);
      errors = {};
    });
  });

  function draft(): LinkPolicy {
    const { visits_used: _, ...rest } = policy;
    return { ...rest, rules: cleanRules(rules) };
  }

  function simulationInput() {
    return {
      policy: draft(),
      query: sampleQuery.replace(/^\?/, ''),
      accept_language: sampleLanguage,
      user_agent: sampleAgent,
      extra_path: policy.forward_path ? samplePath.trim() : '',
    };
  }

  function changeType(rule: RedirectRule, index: number, type: ConditionType) {
    rule.conditions[index] = newCondition(type);
  }

  function move(index: number, by: number) {
    const next = [...rules];
    [next[index], next[index + by]] = [next[index + by], next[index]];
    rules = next;
    errors = {};
    requestAnimationFrame(() => document.getElementById(`${uid}-move-${index + by}-${by < 0 ? 'up' : 'down'}`)?.focus());
  }

  async function addRule() {
    if (rules.length >= MAX_RULES) return;
    rules.push({ target: '', conditions: [newCondition('language')] });
    await tick();
    document.getElementById(`${uid}-${rules.length - 1}-0-type`)?.focus();
  }

  async function removeRule(index: number) {
    rules.splice(index, 1);
    errors = {};
    await tick();
    document.getElementById(`${uid}-add`)?.focus();
  }

  function discard() {
    rules = source ? cleanRules(source.rules) : [];
    errors = {};
    saveError = null;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving || !dirty) return;
    saveError = null;
    errors = validateRules(rules);
    const first = Object.keys(errors)[0];
    if (first) {
      await tick();
      document.getElementById(`${uid}-${first}`)?.focus();
      return;
    }
    saving = true;
    try {
      // Re-read so that only the rules change, even if the edit drawer saved the policy meanwhile.
      const fresh = await app.api<Policy>(path);
      const { visits_used: _, ...rest } = fresh;
      const saved = await app.api<Policy>(path, 'PATCH', { ...rest, rules: cleanRules(rules) });
      base = JSON.stringify(cleanRules(saved.rules));
      onsaved(saved);
      toast(tr('detRulesSaved'), { body: tr(saved.rules.length === 1 ? 'detRulesSavedOne' : 'detRulesSavedMany', { count: saved.rules.length }) });
    } catch (failure) {
      saveError = problemMessage(failure);
      await tick();
      document.getElementById(`${uid}-save-error`)?.focus();
    } finally {
      saving = false;
    }
  }

  async function simulate() {
    if (saving || simulating) return;
    simulating = true;
    simulation = null;
    simulationError = null;
    const input = simulationInput();
    const key = JSON.stringify(input);
    try {
      const result = await app.api<SimulationResult>(`${path}/simulate`, 'POST', input);
      simulation = { key, result };
    } catch (failure) {
      simulationError = problemMessage(failure);
    } finally {
      simulating = false;
    }
  }

  function simulateOnEnter(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void simulate();
    }
  }

  const err = (key: string) => errors[key];
  const errId = (key: string) => (errors[key] ? `${uid}-${key}-error` : undefined);
</script>

<div class="grid-main-side">
  <form class="panel" novalidate onsubmit={save} aria-busy={saving}>
    <header class="panel-head">
      <div class="panel-title"><h2>{tr('redirectRules')}</h2><p>{tr('detRulesHint')}</p></div>
    </header>
    <fieldset class="rules-set" disabled={saving}>
      <legend class="visually-hidden">{tr('redirectRules')}</legend>
      <ol class="rules">
        {#each rules as rule, r (rule)}
          <li class="rule" class:matched={currentSimulation?.rule === r + 1}>
            <span class="rule-n num" aria-hidden="true">{r + 1}</span>
            <fieldset class="rule-body">
              <legend class="visually-hidden">{tr('detRuleNumber', { n: r + 1 })}</legend>
              {#each rule.conditions as c, i (c)}
                <div class="cond">
                  <span class="cond-if">{i === 0 ? tr('detRuleIf') : tr('detRuleAndPrefix')}</span>
                  <div class="cond-fields">
                    <select id="{uid}-{r}-{i}-type" class="select" data-size="sm" aria-label={tr('condition')}
                      value={c.type} onchange={(e) => changeType(rule, i, e.currentTarget.value as ConditionType)}>
                      {#each Object.entries(conditionLabels) as [value, label] (value)}<option {value}>{tr(label)}</option>{/each}
                    </select>
                    {#if c.type === 'query'}
                      <input id="{uid}-{r}-{i}-key" class="input mono key" data-size="sm" maxlength="128" placeholder="campaign"
                        aria-label={tr('queryKey')} aria-invalid={err(`${r}-${i}-key`) ? 'true' : undefined}
                        aria-describedby={errId(`${r}-${i}-key`)} bind:value={c.key} />
                      <span class="cond-op" aria-hidden="true">=</span>
                    {:else}
                      <span class="cond-op">{tr('detRuleIsWord')}</span>
                    {/if}
                    {#if conditionOptions[c.type]}
                      <select class="select" data-size="sm" aria-label={tr('conditionValue')} bind:value={c.value}>
                        {#each conditionOptions[c.type] ?? [] as option (option)}<option value={option}>{optionLabel(option)}</option>{/each}
                      </select>
                    {:else}
                      <input id="{uid}-{r}-{i}-value" class="input mono value" data-size="sm" maxlength="512"
                        placeholder={c.type === 'language' ? 'de' : 'newsletter'} aria-label={tr('conditionValue')}
                        aria-invalid={err(`${r}-${i}-value`) ? 'true' : undefined} aria-describedby={errId(`${r}-${i}-value`)}
                        bind:value={c.value} />
                    {/if}
                    {#if rule.conditions.length > 1}
                      <button type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only
                        aria-label={tr('detConditionRemove', { n: i + 1, rule: r + 1 })} use:tip={tr('removeCondition')}
                        onclick={() => { rule.conditions.splice(i, 1); errors = {}; }}><Icon name="x" /></button>
                    {/if}
                  </div>
                </div>
                {#if err(`${r}-${i}-key`)}<p id="{uid}-{r}-{i}-key-error" class="field-error cond-error"><Icon name="circle-alert" />{tr(err(`${r}-${i}-key`))}</p>{/if}
                {#if err(`${r}-${i}-value`)}<p id="{uid}-{r}-{i}-value-error" class="field-error cond-error"><Icon name="circle-alert" />{tr(err(`${r}-${i}-value`))}</p>{/if}
              {/each}
              <div class="cond">
                <span class="cond-if"></span>
                <button type="button" class="btn" data-variant="ghost" data-size="sm" disabled={rule.conditions.length >= MAX_CONDITIONS}
                  onclick={() => rule.conditions.push(newCondition())}><Icon name="plus" />{tr('addCondition')}</button>
              </div>
              <div class="cond">
                <label class="cond-if" for="{uid}-{r}-target">{tr('detRuleGoTo')}</label>
                <input id="{uid}-{r}-target" class="input mono" data-size="sm" type="url" maxlength="2048" placeholder="https://"
                  aria-invalid={err(`${r}-target`) ? 'true' : undefined} aria-describedby={errId(`${r}-target`)} bind:value={rule.target} />
              </div>
              {#if err(`${r}-target`)}<p id="{uid}-{r}-target-error" class="field-error cond-error"><Icon name="circle-alert" />{tr('invalidTarget')}</p>{/if}
            </fieldset>
            <div class="rule-actions">
              <button id="{uid}-move-{r}-up" type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only
                aria-label={tr('detRuleMoveUp', { n: r + 1 })} use:tip={tr('moveUp')} disabled={r === 0} onclick={() => move(r, -1)}><Icon name="chevron-up" /></button>
              <button id="{uid}-move-{r}-down" type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only
                aria-label={tr('detRuleMoveDown', { n: r + 1 })} use:tip={tr('moveDown')} disabled={r === rules.length - 1} onclick={() => move(r, 1)}><Icon name="chevron-down" /></button>
              <button type="button" class="btn" data-variant="ghost" data-size="sm" data-icon-only
                aria-label={tr('detRuleRemove', { n: r + 1 })} use:tip={tr('removeRule')} onclick={() => removeRule(r)}><Icon name="trash-2" /></button>
            </div>
          </li>
        {/each}
        <li class="rule default" class:matched={currentSimulation?.rule === null}>
          <span class="rule-n" aria-hidden="true"><Icon name="corner-down-right" /></span>
          <div class="rule-body">
            <div class="cond"><span class="cond-if">{tr('detRuleOtherwise')}</span><span class="dest small"><b>{fallback.host}</b>{fallback.rest}</span></div>
          </div>
        </li>
      </ol>
    </fieldset>
    {#if saveError}
      <div class="panel-body save-error"><p id="{uid}-save-error" class="notice" data-tone="error" role="alert" tabindex="-1"><Icon name="circle-alert" />{tr(saveError)}</p></div>
    {/if}
    <footer class="panel-foot">
      <button id="{uid}-add" type="button" class="btn" disabled={saving || rules.length >= MAX_RULES} onclick={addRule}><Icon name="plus" />{tr('addRule')}</button>
      {#if rules.length >= MAX_RULES}<span class="field-hint">{tr('detRulesLimit', { count: MAX_RULES })}</span>{/if}
      <span class="spacer"></span>
      {#if dirty}<button type="button" class="btn" data-variant="ghost" disabled={saving} onclick={discard}>{tr('detDiscard')}</button>{/if}
      <button type="submit" class="btn" data-variant="primary" disabled={saving || !dirty}>{saving ? tr('saving') : tr('detRulesSave')}</button>
    </footer>
  </form>

  <section class="panel" aria-labelledby="{uid}-sim">
    <header class="panel-head">
      <div class="panel-title"><h2 id="{uid}-sim">{tr('detSimTitle')}</h2><p>{tr('detSimHint')}</p></div>
    </header>
    <fieldset class="panel-body sim" disabled={saving || simulating}>
      <legend class="visually-hidden">{tr('detSimTitle')}</legend>
      <div class="field">
        <label class="field-label" for="{uid}-preset">{tr('detSimVisitor')}</label>
        <select id="{uid}-preset" class="select" value={agents.find((a) => a[1] === sampleAgent)?.[1] ?? ''}
          onchange={(e) => { if (e.currentTarget.value) sampleAgent = e.currentTarget.value; }}>
          <option value="">{tr('detSimVisitorCustom')}</option>
          {#each agents as [label, agent] (label)}<option value={agent}>{tr(label)}</option>{/each}
        </select>
      </div>
      <div class="field">
        <label class="field-label" for="{uid}-agent">{tr('simulateAgent')}</label>
        <textarea id="{uid}-agent" class="textarea mono" rows="3" maxlength="1024" bind:value={sampleAgent} onkeydown={simulateOnEnter}></textarea>
      </div>
      <div class="field">
        <label class="field-label" for="{uid}-lang">{tr('simulateLanguage')}</label>
        <input id="{uid}-lang" class="input mono" maxlength="256" placeholder="de-CH,de;q=0.9,en;q=0.8" bind:value={sampleLanguage} onkeydown={simulateOnEnter} />
      </div>
      <div class="field">
        <label class="field-label" for="{uid}-query">{tr('simulateQuery')}</label>
        <input id="{uid}-query" class="input mono" maxlength="4097" placeholder="campaign=newsletter" bind:value={sampleQuery} onkeydown={simulateOnEnter} />
      </div>
      {#if policy.forward_path}
        <div class="field">
          <label class="field-label" for="{uid}-path">{tr('simulatePath')}</label>
          <input id="{uid}-path" class="input mono" maxlength="199" placeholder="guide/pdf" bind:value={samplePath} onkeydown={simulateOnEnter} />
        </div>
      {/if}
      <div><button type="button" class="btn" onclick={simulate}><Icon name={simulating ? 'loader-circle' : 'play'} />{simulating ? tr('loading') : tr('runSimulation')}</button></div>
      {#if simulationError}<p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(simulationError)}</p>{/if}
      <div class="sim-result" aria-live="polite">
        {#if currentSimulation}
          <span class="muted">{currentSimulation.rule === null ? tr('detSimDefault') : tr('detSimRule', { n: currentSimulation.rule })} · {tr('detSimStatus', { status: currentSimulation.status })}</span>
          <span class="mono target">{currentSimulation.target}</span>
        {:else}
          <span class="muted">{tr('detSimNoResult')}</span>
        {/if}
      </div>
      <p class="field-hint">{tr('ruleSimulatorHint')}</p>
    </fieldset>
  </section>
</div>

<style>
  .rules-set,
  .sim {
    margin: 0;
    border: 0;
    min-inline-size: 0;
  }
  .rules-set {
    padding: 0;
  }
  .rules {
    display: grid;
    padding: var(--space-2);
  }
  .rule {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    border-radius: var(--radius-md);
  }
  .rule + .rule {
    border-top: 1px solid var(--border-subtle);
  }
  .rule.matched {
    background: var(--accent-soft);
  }
  .rule-n {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-xs);
    background: var(--neutral-soft);
    color: var(--text-secondary);
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
  }
  .default .rule-n {
    color: var(--text-muted);
  }
  .rule-n :global(svg) {
    width: 13px;
    height: 13px;
  }
  .rule-body {
    display: grid;
    gap: 6px;
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .cond {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .cond-fields {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .cond .select {
    width: auto;
  }
  .cond .input {
    flex: 1;
    min-width: 0;
  }
  .cond-fields .input.key {
    flex: 0 1 140px;
  }
  .cond-fields .input.value {
    flex: 0 1 180px;
  }
  .cond-if {
    width: 64px;
    flex: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .cond-op {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .cond-error {
    margin-left: calc(64px + 6px);
  }
  .small {
    font-size: var(--text-sm);
    min-width: 0;
  }
  .rule-actions {
    display: flex;
    gap: 2px;
  }
  .panel-foot > .spacer {
    flex: 1;
  }
  .save-error {
    padding-top: 0;
  }
  .sim {
    display: grid;
    gap: var(--space-3);
  }
  .sim-result {
    display: grid;
    gap: 4px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    font-size: var(--text-xs);
  }
  .target {
    overflow-wrap: anywhere;
  }
  @media (max-width: 640px) {
    .rule {
      flex-wrap: wrap;
    }
    .rule-body {
      flex-basis: calc(100% - 24px - var(--space-3));
    }
    .rule-actions {
      margin-left: calc(24px + var(--space-3));
    }
    .cond {
      flex-wrap: wrap;
    }
    .cond-if {
      width: 100%;
    }
    .cond-error {
      margin-left: 0;
    }
  }
</style>
