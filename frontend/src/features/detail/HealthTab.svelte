<script lang="ts">
  // Destination health of one link: enrollment, priority, and a manual check through the monitoring
  // endpoints. The server keeps only the latest result per link, so there is no check history.
  import { untrack } from 'svelte';
  import { ApiError } from '../../api';
  import type { LinkMonitorSummary, MonitorStatus } from '../../api';
  import type { Messages } from '../../i18n';
  import { problemMessage } from '../../format';
  import Icon from '../../lib/Icon.svelte';
  import Badge from '../../lib/ui/Badge.svelte';
  import { app } from '../../lib/app.svelte';
  import { ago, num, when } from '../../lib/fmt';
  import { tr } from '../../lib/locale.svelte';
  import { href } from '../../lib/router.svelte';
  import { health, healthState } from '../../lib/status';
  import { toast } from '../../lib/toast.svelte';

  type Failure = 'blocked' | 'busy' | 'dns' | 'timeout' | 'connection' | 'protocol';
  interface Item {
    id: number;
    priority: boolean;
    status: MonitorStatus;
    last_checked: number | null;
    http_status: number | null;
    elapsed_ms: number | null;
    redirects?: number | null;
    failure: string | null;
    consecutive_failures?: number;
  }
  interface Report { enabled: boolean; items: Item[]; next_after: number | null }

  let { linkId, monitor, onchanged }: {
    linkId: number;
    /** From the link record; undefined when the server does not provide the field. */
    monitor: LinkMonitorSummary | null | undefined;
    onchanged: () => void;
  } = $props();

  const failures: Record<Failure, keyof Messages> = {
    blocked: 'monitorBlocked', busy: 'monitorBusy', dns: 'monitorDns', timeout: 'monitorTimeout',
    connection: 'monitorConnection', protocol: 'monitorProtocol',
  };
  const statusLabels: Record<MonitorStatus, keyof Messages> = {
    not_checked: 'monitorNotChecked', healthy: 'monitorHealthy', redirected: 'monitorRedirected',
    warning: 'monitorWarning', unreachable: 'monitorUnreachable', broken: 'monitorBroken', unknown: 'monitorUnknown',
  };

  let enabled = $state<boolean | null>(null);
  let item = $state.raw<Item | null | undefined>(undefined);
  let loading = $state(false);
  let working = $state(false);
  let error = $state<keyof Messages | null>(null);
  const view = $derived(healthState(health(item?.status)));

  $effect(() => {
    const id = linkId;
    const known = monitor;
    untrack(() => void load(id, known));
  });

  /** Reads whether monitoring is on and, for servers without the link's `monitor` field, finds the entry. */
  async function load(id: number, known: LinkMonitorSummary | null | undefined) {
    loading = true;
    error = null;
    try {
      let report = await app.api<Report>('/monitoring');
      enabled = report.enabled;
      if (known !== undefined) {
        item = known ? { id, ...known } : null;
        return;
      }
      let found = report.items.find((entry) => entry.id === id);
      for (let page = 0; !found && report.next_after && page < 40; page++) {
        report = await app.api<Report>(`/monitoring?after=${report.next_after}`);
        found = report.items.find((entry) => entry.id === id);
      }
      item = found ?? null;
    } catch (failure) {
      error = problemMessage(failure);
    } finally {
      loading = false;
    }
  }

  async function run(action: () => Promise<void>) {
    if (working) return;
    working = true;
    error = null;
    try {
      await action();
    } catch (issue) {
      if (issue instanceof ApiError && issue.code === 'monitoring_busy') error = 'monitorBusy';
      else if (issue instanceof ApiError && issue.code === 'monitoring_link_inactive') error = 'monitorLinkInactive';
      else error = problemMessage(issue);
    } finally {
      working = false;
    }
  }

  function enroll(on: boolean, priority = false) {
    const wasEnrolled = item != null;
    return run(async () => {
      await app.api<void>(`/monitoring/links/${linkId}`, 'PATCH', { enabled: on, priority });
      const base: Item = item ?? { id: linkId, priority, status: 'not_checked', last_checked: null, http_status: null, elapsed_ms: null, failure: null };
      item = on ? { ...base, priority } : null;
      toast(!on ? tr('detMonitorOff') : wasEnrolled ? tr('detMonitorUpdated') : tr('detMonitorOn'));
      onchanged();
    });
  }

  function check() {
    return run(async () => {
      item = await app.api<Item>(`/monitoring/links/${linkId}/check`, 'POST');
      toast(tr('detCheckDone'), { body: tr(statusLabels[item.status]) });
      onchanged();
    });
  }

  const failureText = (code: string | null) => (code && code in failures ? tr(failures[code as Failure]) : code ? tr('monitorUnknown') : '');
</script>

<section class="panel" aria-busy={loading || working}>
  <header class="panel-head">
    <div class="panel-title">
      <h2>{tr('detHealthTitle')}</h2>
      <p>{item ? (item.priority ? tr('detHealthPriorityHint') : tr('detHealthHint')) : tr('detHealthOffHint')}</p>
    </div>
    {#if item && enabled}
      <span class="spacer"></span>
      <button type="button" class="btn" data-size="sm" disabled={working} onclick={check}>
        <Icon name={working ? 'loader-circle' : 'refresh-cw'} />{tr('monitorCheck')}
      </button>
    {/if}
  </header>
  <div class="panel-body stack">
    {#if error}<p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(error)}</p>{/if}
    {#if loading && item === undefined}
      <p class="muted" role="status">{tr('loading')}</p>
    {:else if enabled === false}
      <div class="notice" data-tone="neutral"><Icon name="info" /><p>{tr('monitorOff')} {tr('opsOffInSettings')} <a href={href('settings', undefined, { section: 'operations' })}>{tr('opsOpenSettings')}</a>.</p></div>
    {/if}
    {#if item === null && enabled}
      <div><button type="button" class="btn" disabled={working} onclick={() => enroll(true)}><Icon name="activity" />{tr('detMonitorStart')}</button></div>
    {:else if item}
      <dl class="facts">
        <dt>{tr('status')}</dt><dd><Badge state={view} /></dd>
        <dt>{tr('detLastChecked')}</dt>
        <dd>{#if item.last_checked}<time datetime={new Date(item.last_checked * 1000).toISOString()}>{when(item.last_checked)}</time> <span class="muted">({ago(item.last_checked)})</span>{:else}{tr('monitorNotChecked')}{/if}</dd>
        {#if item.http_status}<dt>{tr('detHttpStatus')}</dt><dd class="mono">{item.http_status}</dd>{/if}
        {#if item.elapsed_ms !== null}<dt>{tr('detResponseTime')}</dt><dd class="num">{tr('detMilliseconds', { value: num(item.elapsed_ms) })}</dd>{/if}
        {#if item.redirects}<dt>{tr('detRedirects')}</dt><dd class="num">{num(item.redirects)}</dd>{/if}
        {#if item.failure}<dt>{tr('detFailure')}</dt><dd>{failureText(item.failure)}</dd>{/if}
        {#if item.consecutive_failures}<dt>{tr('detConsecutiveFailures')}</dt><dd class="num">{num(item.consecutive_failures)}</dd>{/if}
      </dl>
      <label class="check">
        <input class="switch" type="checkbox" checked={item.priority} disabled={working || !enabled}
          onchange={(e) => enroll(true, e.currentTarget.checked)} />
        <span>{tr('monitorPriority')}<small>{tr('detPriorityHint')}</small></span>
      </label>
      <div><button type="button" class="btn" data-variant="danger-quiet" disabled={working} onclick={() => enroll(false)}><Icon name="circle-pause" />{tr('monitorRemove')}</button></div>
    {/if}
  </div>
</section>
