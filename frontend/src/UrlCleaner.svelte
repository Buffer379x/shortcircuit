<script lang="ts">
  import type { Messages } from './i18n';
  import { proposeCleanUrl } from './url-cleaner';

  let { t, value, onapply }: { t: Messages; value: string; onapply: (value: string) => void } = $props();

  let show = $state(false);
  let parameters = $state('');
  let exceptions = $state('');

  function list(value: string): Set<string> {
    return new Set(value.split(',').map((item) => item.trim().toLowerCase()).filter(Boolean));
  }

  const source = $derived(value.trim());
  const proposal = $derived(proposeCleanUrl(source, parameters.trim() ? list(parameters) : undefined, list(exceptions)));
</script>

{#if source}
  <div class="stack compact">
    <div><button type="button" class="button" aria-expanded={show} onclick={() => show = !show}>{t.urlCleaner}</button></div>
    {#if show}
      <section class="panel panel-pad stack" aria-label={t.urlCleaner}>
        <details>
          <summary>{t.cleanerOptions}</summary>
          <div class="option-fields stack compact">
            <label class="field">
              <span>{t.cleanerNames}</span><input type="text" maxlength="512" bind:value={parameters} />
              <small>{t.cleanerNamesHint}</small>
            </label>
            <label class="field"><span>{t.cleanerExceptions}</span><input type="text" maxlength="512" bind:value={exceptions} /></label>
          </div>
        </details>
        {#if proposal.status === 'changes'}
          <dl class="stack compact">
            <div><dt>{t.cleanerOriginal}</dt><dd class="mono">{source}</dd></div>
            <div><dt>{t.cleanerPreview}</dt><dd class="mono">{proposal.cleaned}</dd></div>
            <div><dt>{t.cleanerRemoved}</dt><dd>{proposal.removed.join(', ')}</dd></div>
          </dl>
          <div>
            <button class="button" type="button" onclick={() => {
              if (proposal.status === 'changes') {
                onapply(proposal.cleaned);
                show = false;
              }
            }}>{t.cleanerApply}</button>
          </div>
        {:else}
          <p role="status">{t[proposal.status === 'protected' ? 'cleanerProtected'
            : proposal.status === 'invalid' ? 'invalidTarget' : 'cleanerUnchanged']}</p>
        {/if}
      </section>
    {/if}
  </div>
{/if}

<style>
  .option-fields { padding-block-start: 8px; }
  dl, dd { margin: 0; overflow-wrap: anywhere; }
  dt { font-weight: 600; }
</style>
