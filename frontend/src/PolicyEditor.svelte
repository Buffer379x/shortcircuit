<script lang="ts">
  import { onMount } from 'svelte';
  import type { Api, ConditionType, Link, LinkPolicy, Policy, RedirectCondition, RedirectRule, SimulationResult } from './api';
  import Dialog from './Dialog.svelte';
  import { problemMessage } from './format';
  import type { Messages } from './i18n';

  let { t, api, link, onclose, onsaved }: {
    t: Messages; api: Api; link: Link; onclose: () => void; onsaved: () => void;
  } = $props();

  const queryCondition = (): RedirectCondition => ({ type: 'query', key: '', value: '' });
  const types = {
    language: 'matchLanguage', device: 'matchDevice', os: 'matchOs', browser: 'matchBrowser', query: 'matchQuery',
  } satisfies Record<ConditionType, keyof Messages>;
  const options: Partial<Record<ConditionType, string[]>> = {
    device: ['desktop', 'mobile', 'tablet', 'unknown'],
    os: ['windows', 'macos', 'linux', 'android', 'ios', 'chromeos', 'unknown'],
    browser: ['edge', 'firefox', 'chrome', 'safari', 'opera', 'unknown'],
  };
  const path = $derived(`/links/${link.id}/policy`);
  let status = $state(302);
  let starts = $state('');
  let expires = $state('');
  let maximum = $state<number>();
  let used = $state(0);
  let forwardQuery = $state(false);
  let forwardPath = $state(false);
  let noindex = $state(false);
  let rules = $state<RedirectRule[]>([]);
  let loading = $state(true);
  let ready = $state(false);
  let saving = $state(false);
  let error = $state<keyof Messages | null>(null);
  let sampleQuery = $state('');
  let sampleLanguage = $state('');
  let sampleAgent = $state('');
  let samplePath = $state('');
  let simulating = $state(false);
  let simulationError = $state<keyof Messages | null>(null);
  let simulation = $state<{ key: string; result: SimulationResult } | null>(null);
  const currentSimulation = $derived(simulation && simulation.key === JSON.stringify(simulationInput()) ? simulation.result : null);

  onMount(() => { void load(); });

  function asLocal(seconds: number | null): string {
    if (seconds === null) return '';
    const date = new Date(seconds * 1000);
    if (Number.isNaN(date.getTime())) return '';
    return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 19);
  }

  function asTimestamp(value: string): number | null {
    return value ? Math.floor(new Date(value).getTime() / 1000) : null;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const policy = await api<Policy>(path);
      status = policy.status;
      starts = asLocal(policy.starts_at);
      expires = asLocal(policy.expires_at);
      maximum = policy.max_visits ?? undefined;
      used = policy.visits_used;
      forwardQuery = policy.forward_query;
      forwardPath = policy.forward_path;
      noindex = policy.noindex ?? false;
      rules = policy.rules;
      ready = true;
    } catch (result) {
      error = problemMessage(result);
    } finally {
      loading = false;
    }
  }

  function changeType(rule: RedirectRule, index: number, next: ConditionType) {
    rule.conditions[index] = next === 'query' ? queryCondition() : { type: next, value: options[next]?.[0] ?? '' };
  }

  function moveRule(index: number, offset: number) {
    const moved = [...rules];
    const [item] = moved.splice(index, 1);
    moved.splice(index + offset, 0, item);
    rules = moved;
  }

  function policyDraft(): LinkPolicy {
    return {
      status, starts_at: asTimestamp(starts), expires_at: asTimestamp(expires), max_visits: maximum ?? null,
      forward_query: forwardQuery, forward_path: forwardPath, noindex, rules,
    };
  }

  function simulationInput() {
    return {
      policy: policyDraft(),
      query: sampleQuery.replace(/^\?/, ''),
      accept_language: sampleLanguage,
      user_agent: sampleAgent,
      extra_path: forwardPath ? samplePath.trim() : '',
    };
  }

  async function simulate() {
    if (!ready || saving || simulating) return;
    simulating = true;
    simulation = null;
    simulationError = null;
    const input = simulationInput();
    const key = JSON.stringify(input);
    try {
      const result = await api<SimulationResult>(`${path}/simulate`, 'POST', input);
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

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (saving || !ready) return;
    error = null;
    const draft = policyDraft();
    const { starts_at: start, expires_at: end } = draft;
    if ((start !== null && !Number.isFinite(start)) || (end !== null && !Number.isFinite(end))) {
      error = 'invalidRedirectWindow';
      return;
    }
    saving = true;
    try {
      await api<Policy>(path, 'PATCH', draft);
      onsaved();
    } catch (result) {
      error = problemMessage(result);
    } finally {
      saving = false;
    }
  }
</script>

<Dialog {t} title={t.redirectPolicy} open={true} busy={saving} {onclose}>
  <form onsubmit={save} aria-busy={loading || saving}>
    <div class="dialog-body stack">
      {#if !ready}
        {#if loading}<p role="status">{t.loading}</p>{/if}
        {#if error}
          <p class="notice error" role="alert">{t[error]}</p>
          <div><button class="button" type="button" onclick={load}>{t.retry}</button></div>
        {/if}
      {:else}
        <fieldset disabled={saving} class="stack">
          <legend class="visually-hidden">{t.redirectPolicy}</legend>
          <label class="field">
            <span>{t.redirectStatus}</span>
            <select bind:value={status}>
              <option value={302}>302 · {t.temporary}</option><option value={307}>307 · {t.temporary}</option>
              <option value={301}>301 · {t.permanent}</option><option value={308}>308 · {t.permanent}</option>
            </select>
            {#if status === 301 || status === 308}<small>{t.permanentHint}</small>{/if}
          </label>
          <label class="field"><span>{t.redirectStarts}</span><input type="datetime-local" step="1" bind:value={starts} /></label>
          <label class="field"><span>{t.redirectExpires}</span><input type="datetime-local" step="1" bind:value={expires} /></label>
          <label class="field"><span>{t.visitLimit}</span><input type="number" min="1" max={Number.MAX_SAFE_INTEGER} step="1" bind:value={maximum} /><small>{t.visitsUsed} · {used}</small></label>
          <label class="row"><input type="checkbox" bind:checked={forwardQuery} />{t.forwardQuery}</label>
          <label class="row"><input type="checkbox" bind:checked={forwardPath} />{t.forwardPath}</label>
          <label class="field">
            <span class="row"><input type="checkbox" bind:checked={noindex} />{t.requestNoIndex}</span>
            <small>{t.requestNoIndexHint}</small>
          </label>
          <div class="stack compact">
            <h3>{t.redirectRules}</h3>
            <p>{t.ruleHint}</p>
          </div>
          {#each rules as rule, index}
            <fieldset class="rule stack compact">
              <legend>{t.rule} {index + 1}</legend>
              <label class="field"><span>{t.target}</span><input type="url" required bind:value={rule.target} /></label>
              {#each rule.conditions as condition, conditionIndex}
                <div class="condition stack compact">
                  <label class="field"><span>{t.condition}</span>
                    <select value={condition.type} onchange={(event) => changeType(rule, conditionIndex, event.currentTarget.value as ConditionType)}>
                      {#each Object.entries(types) as [value, label] (value)}<option {value}>{t[label]}</option>{/each}
                    </select>
                  </label>
                  {#if condition.type === 'query'}
                    <label class="field"><span>{t.queryKey}</span><input required maxlength="128" bind:value={condition.key} /></label>
                  {/if}
                  <label class="field">
                    <span>{t.conditionValue}</span>
                    {#if options[condition.type]}
                      <select bind:value={condition.value}>
                        {#each options[condition.type] ?? [] as option (option)}
                          <option value={option}>{option === 'unknown' ? t.unknown : option}</option>
                        {/each}
                      </select>
                    {:else}
                      <input required={condition.type !== 'query'} placeholder={condition.type === 'language' ? 'en' : undefined} maxlength="512" bind:value={condition.value} />
                    {/if}
                  </label>
                  {#if rule.conditions.length > 1}<div><button class="button" type="button" onclick={() => rule.conditions.splice(conditionIndex, 1)}>{t.removeCondition}</button></div>{/if}
                </div>
              {/each}
              <div class="rule-actions row">
                <button class="button" type="button" disabled={rule.conditions.length >= 8} onclick={() => rule.conditions.push(queryCondition())}>{t.addCondition}</button>
                <button class="button" type="button" disabled={index === 0} onclick={() => moveRule(index, -1)}>{t.moveUp}</button>
                <button class="button" type="button" disabled={index + 1 === rules.length} onclick={() => moveRule(index, 1)}>{t.moveDown}</button>
                <button class="button" type="button" onclick={() => rules.splice(index, 1)}>{t.removeRule}</button>
              </div>
            </fieldset>
          {/each}
          <div><button type="button" class="button" disabled={rules.length >= 16} onclick={() => rules.push({ target: '', conditions: [queryCondition()] })}>{t.addRule}</button></div>
        </fieldset>
        <details class="rule">
          <summary>{t.ruleSimulator}</summary>
          <fieldset disabled={saving || simulating} class="stack compact simulator">
            <legend class="visually-hidden">{t.ruleSimulator}</legend>
            <p>{t.ruleSimulatorHint}</p>
            <label class="field"><span>{t.simulateQuery}</span><input bind:value={sampleQuery} maxlength="4097" placeholder="lang=de" onkeydown={simulateOnEnter} /></label>
            <label class="field"><span>{t.simulateLanguage}</span><input bind:value={sampleLanguage} maxlength="256" placeholder="de-DE,en;q=0.8" onkeydown={simulateOnEnter} /></label>
            <label class="field"><span>{t.simulateAgent}</span><input bind:value={sampleAgent} maxlength="1024" onkeydown={simulateOnEnter} /></label>
            {#if forwardPath}
              <label class="field"><span>{t.simulatePath}</span><input bind:value={samplePath} maxlength="199" placeholder="Guide/PDF" onkeydown={simulateOnEnter} /></label>
            {/if}
            <div><button type="button" class="button" onclick={simulate}>{simulating ? t.loading : t.runSimulation}</button></div>
          </fieldset>
          {#if simulationError}<p class="notice error" role="alert">{t[simulationError]}</p>{/if}
          {#if currentSimulation}
            <div class="notice stack compact" role="status">
              <strong>{currentSimulation.status} · {currentSimulation.rule === null ? t.simulationFallback : `${t.rule} ${currentSimulation.rule}`}</strong>
              <code>{currentSimulation.target}</code>
            </div>
          {/if}
        </details>
        {#if error}<p class="notice error" role="alert">{t[error]}</p>{/if}
      {/if}
    </div>
    <footer class="dialog-foot row">
      <button class="button" type="button" disabled={saving} onclick={onclose}>{t.cancel}</button>
      {#if ready}<button class="button primary" type="submit" disabled={saving}>{saving ? t.saving : t.save}</button>{/if}
    </footer>
  </form>
</Dialog>

<style>
  fieldset { margin: 0; padding: 0; border: 0; min-inline-size: 0; }
  .rule { border: 1px solid var(--line); border-radius: 8px; padding: 12px; }
  .condition { padding-block: 8px; border-block-end: 1px solid var(--line); }
  .rule-actions { flex-wrap: wrap; }
  .simulator { margin-block: 10px; }
  h3 { font-size: 1rem; }
</style>
