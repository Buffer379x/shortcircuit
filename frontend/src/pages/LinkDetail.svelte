<script lang="ts">
  // One short link: hero with its address and actions, then tabs for the overview, analytics,
  // redirect rules, access and schedule, destination health, and history. The tab lives in ?tab=.
  import { onDestroy, untrack } from 'svelte';
  import type { CatalogLink, LinkOrganization, LinkProtection, Policy } from '../api';
  import type { Messages } from '../i18n';
  import { problemMessage } from '../format';
  import Icon from '../lib/Icon.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import CopyButton from '../lib/ui/CopyButton.svelte';
  import Empty from '../lib/ui/Empty.svelte';
  import LinkPreview from '../lib/ui/LinkPreview.svelte';
  import Menu from '../lib/ui/Menu.svelte';
  import TagChip from '../lib/ui/TagChip.svelte';
  import TimeChart from '../lib/charts/TimeChart.svelte';
  import { app, loadPreviewSetting, shortUrl } from '../lib/app.svelte';
  import { ago, date, day, num, splitUrl, when } from '../lib/fmt';
  import { tr } from '../lib/locale.svelte';
  import { linksChanged, openCreate, overlays } from '../lib/overlays.svelte';
  import { href, route, setParams } from '../lib/router.svelte';
  import { health, healthState, linkState } from '../lib/status';
  import { toast } from '../lib/toast.svelte';
  import RouteMap from '../features/RouteMap.svelte';
  import AnalyticsTab from '../features/detail/AnalyticsTab.svelte';
  import HealthTab from '../features/detail/HealthTab.svelte';
  import HistoryTab from '../features/detail/HistoryTab.svelte';
  import RulesTab from '../features/detail/RulesTab.svelte';
  import { series, type Summary } from '../features/detail/analytics';
  import { describeRule } from '../features/detail/rules';
  import { fetchLink } from '../features/detail/link';

  let { id }: { id: number } = $props();

  type Tab = 'overview' | 'analytics' | 'rules' | 'access' | 'health' | 'history';
  const TAB_KEYS: Tab[] = ['overview', 'analytics', 'rules', 'access', 'health', 'history'];
  const initial = route.params.get('tab') as Tab;
  let tab = $state<Tab>(TAB_KEYS.includes(initial) ? initial : 'overview');
  $effect(() => setParams({ tab: tab === 'overview' ? null : tab }));

  let link = $state.raw<CatalogLink | null>(null);
  let loadError = $state<keyof Messages | null>(null);
  let missing = $state(false);
  let policy = $state.raw<Policy | null>(null);
  let policyError = $state<keyof Messages | null>(null);
  let protection = $state<'password' | 'one_time' | null | undefined>(undefined);
  let consumed = $state(false);
  let summary = $state.raw<Summary | null>(null);
  let summaryError = $state<keyof Messages | null>(null);
  let historyCount = $state<number | null>(null);
  let working = $state(false);
  let request = 0;
  // Preview refresh: the fetch runs in the background, so the page polls briefly for the result.
  let previewBusy = $state(false);
  let previewWaiting = $state(false);
  let previewPoll: ReturnType<typeof setTimeout> | undefined;
  void loadPreviewSetting();

  const uid = $props.id();
  const url = $derived(link ? shortUrl(link) : '');
  const st = $derived(link ? linkState(link) : null);
  const hs = $derived(link ? health(link.monitor?.status) : 'unchecked');
  const dest = $derived(splitUrl(link?.target ?? ''));
  const name = $derived(link ? link.title || `/${link.slug}` : '');
  const startsAt = $derived(policy?.starts_at ?? link?.starts_at ?? null);
  const expiresAt = $derived(policy?.expires_at ?? link?.expires_at ?? null);
  const maxVisits = $derived(policy?.max_visits ?? link?.max_visits ?? null);
  const visitsUsed = $derived(policy?.visits_used ?? link?.visits_used ?? 0);
  const ruleCount = $derived(policy?.rules.length ?? link?.rule_count);
  const visits = $derived(summary ? series(summary) : null);
  const timeZone = (() => {
    try {
      return localStorage.getItem('shortcircuit-timezone') || Intl.DateTimeFormat().resolvedOptions().timeZone;
    } catch {
      return Intl.DateTimeFormat().resolvedOptions().timeZone;
    }
  })();

  const tabs = $derived<[Tab, string, number | null | undefined][]>([
    ['overview', tr('detTabOverview'), null],
    ['analytics', tr('detTabAnalytics'), null],
    ['rules', tr('detTabRules'), ruleCount || null],
    ['access', tr('detTabAccess'), null],
    ['health', tr('detTabHealth'), null],
    ['history', tr('detTabHistory'), historyCount || null],
  ]);

  const checks = $derived(
    [
      protection === 'password' ? tr('protectionPassword') : protection === 'one_time' ? tr('protectionOneTime') : null,
      startsAt || expiresAt ? tr('detRouteWindow') : null,
      maxVisits ? tr('detRouteLimit', { count: num(maxVisits) }) : null,
    ].filter((c): c is string => c !== null),
  );

  // Reload everything when the id changes or another view changed links (for example the edit drawer).
  $effect(() => {
    const current = id;
    void overlays.linksVersion;
    untrack(() => void loadAll(current));
  });

  onDestroy(() => {
    app.crumb = '';
    clearTimeout(previewPoll);
  });

  async function loadAll(current: number) {
    const mine = ++request;
    if (link?.id !== current) {
      link = null;
      policy = null;
      summary = null;
      protection = undefined;
      historyCount = null;
    }
    loadError = null;
    missing = false;
    try {
      const next = await fetchLink(current);
      if (mine !== request) return;
      if (!next) {
        missing = true;
        link = null;
        app.crumb = '';
        return;
      }
      link = next;
      app.crumb = next.title || `/${next.slug}`;
    } catch (failure) {
      if (mine === request) loadError = problemMessage(failure);
      return;
    }
    void loadPolicy(current, mine);
    void loadSummary(current, mine);
  }

  async function loadPolicy(current: number, mine: number) {
    policyError = null;
    try {
      const next = await app.api<Policy>(`/links/${current}/policy`);
      if (mine === request) policy = next;
    } catch (failure) {
      if (mine === request) policyError = problemMessage(failure);
    }
    const known = link?.protection;
    if (known !== undefined) {
      protection = known;
      return;
    }
    try {
      const value = await app.api<LinkProtection>(`/links/${current}/protection`);
      if (mine !== request) return;
      protection = value.password_required ? 'password' : value.one_time ? 'one_time' : null;
      consumed = value.consumed;
    } catch {
      /* Protection only adds a badge and a route check; the edit drawer shows its own errors. */
    }
  }

  async function loadSummary(current: number, mine: number) {
    summaryError = null;
    if (link && !link.analytics_enabled) {
      summary = null;
      return;
    }
    try {
      const next = await app.api<Summary>(`/analytics/summary?${new URLSearchParams({ days: '30', link: String(current) })}`);
      if (mine === request) summary = next;
    } catch (failure) {
      if (mine === request) summaryError = problemMessage(failure);
    }
  }

  function retrySummary() {
    if (link) void loadSummary(link.id, request);
  }

  function tabKey(e: KeyboardEvent) {
    const i = TAB_KEYS.indexOf(tab);
    const next = e.key === 'ArrowRight' ? (i + 1) % TAB_KEYS.length : e.key === 'ArrowLeft' ? (i - 1 + TAB_KEYS.length) % TAB_KEYS.length
      : e.key === 'Home' ? 0 : e.key === 'End' ? TAB_KEYS.length - 1 : -1;
    if (next < 0) return;
    tab = TAB_KEYS[next];
    requestAnimationFrame(() => document.getElementById(`${uid}-tab-${tab}`)?.focus());
    e.preventDefault();
  }

  async function act(action: (current: CatalogLink) => Promise<string>) {
    if (!link || working) return;
    working = true;
    try {
      toast(await action(link));
      linksChanged();
    } catch (failure) {
      const message = problemMessage(failure);
      toast(tr(message), { tone: 'error' });
      if (message === 'linkOrganizationChanged') linksChanged();
    } finally {
      working = false;
    }
  }

  const toggleEnabled = () => act(async (current) => {
    await app.api(`/links/${current.id}`, 'PATCH', { enabled: !current.enabled });
    return current.enabled ? tr('detDisabledToast') : tr('detEnabledToast');
  });

  const organize = (change: Partial<LinkOrganization>, message: keyof Messages) => act(async (current) => {
    await app.api<LinkOrganization>(`/links/${current.id}/organization`, 'PATCH', { key: current.selection_key, ...change });
    return tr(message);
  });

  async function refreshPreview() {
    if (!link || previewBusy) return;
    const current = link.id;
    previewBusy = true;
    try {
      await app.api(`/links/${current}/preview/refresh`, 'POST');
      toast(tr('pvQueued'));
      previewWaiting = true;
      link = { ...link, preview: null };
      pollPreview(current, 0);
    } catch (failure) {
      toast(tr(problemMessage(failure)), { tone: 'error' });
    } finally {
      previewBusy = false;
    }
  }

  function pollPreview(current: number, attempt: number) {
    clearTimeout(previewPoll);
    if (attempt >= 10) {
      previewWaiting = false;
      return;
    }
    previewPoll = setTimeout(async () => {
      try {
        const next = await fetchLink(current);
        if (!next || link?.id !== current) return;
        if (next.preview) {
          link = { ...link, preview: next.preview };
          previewWaiting = false;
          return;
        }
      } catch {
        /* Keep polling; the next attempt or a reload shows the result. */
      }
      pollPreview(current, attempt + 1);
    }, 3000);
  }

  function onRulesSaved(saved: Policy) {
    policy = saved;
    linksChanged();
  }
</script>

<div class="page">
  {#if missing}
    <Empty icon="link-2-off" title={tr('detNotFound')}>
      {#snippet actions()}<a class="btn" href={href('links')}><Icon name="link" />{tr('detBackToLinks')}</a>{/snippet}
      {tr('detNotFoundHint')}
    </Empty>
  {:else if loadError && !link}
    <div class="stack">
      <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(loadError)}</p>
      <div><button type="button" class="btn" onclick={() => loadAll(id)}><Icon name="refresh-cw" />{tr('retry')}</button></div>
    </div>
  {:else if !link || !st}
    <p class="muted" role="status">{tr('loading')}</p>
  {:else}
    <section class="hero panel" aria-labelledby="{uid}-name">
      <div class="hero-main">
        <div class="badges">
          <Badge state={st} />
          {#if hs !== 'unchecked'}<Badge state={healthState(hs)} />{/if}
          {#if protection}<span class="badge" data-tone="accent"><Icon name="lock" />{protection === 'password' ? tr('protectionPassword') : tr('protectionOneTime')}</span>{/if}
          {#if link.favorite}<span class="badge"><Icon name="star" />{tr('favorite')}</span>{/if}
        </div>
        <h1 id="{uid}-name" tabindex="-1">{name}</h1>
        <div class="hero-url">
          <span class="short"><span class="host">{link.host}/</span><span class="slug">{link.slug}</span></span>
          <CopyButton value={url} size="md" />
        </div>
        <a class="hero-dest" href={link.target} target="_blank" rel="noreferrer noopener">
          <Icon name="corner-down-right" /><span class="truncate"><b>{dest.host}</b>{dest.rest}</span><Icon name="external-link" />
          <span class="visually-hidden">{tr('detOpensNewTab')}</span>
        </a>
        {#if link.description}<p class="secondary description">{link.description}</p>{/if}
        {#if link.tags.length}<div class="tags">{#each link.tags as t (t)}<TagChip name={t} />{/each}</div>{/if}
      </div>
      <div class="hero-actions btn-group">
        <CopyButton value={url} variant="secondary" size="md" text={tr('copy')} />
        <button type="button" class="btn hide-sm" onclick={() => (overlays.qrId = link!.id)}><Icon name="qr-code" />{tr('qrCode')}</button>
        <button type="button" class="btn" onclick={() => (overlays.editId = link!.id)}><Icon name="pencil" />{tr('detEdit')}</button>
        <Menu label={tr('moreActions')} icon="ellipsis" variant="secondary">
          {#snippet children(close)}
            <a class="menu-item" role="menuitem" href={url} target="_blank" rel="noreferrer noopener" onclick={() => close()}><Icon name="external-link" />{tr('detOpenShort')}</a>
            <button class="menu-item" role="menuitem" onclick={() => { close(); overlays.qrId = link!.id; }}><Icon name="qr-code" />{tr('qrCode')}</button>
            <a class="menu-item" role="menuitem" href={href('qr', link?.id)} onclick={() => close()}><Icon name="palette" />{tr('detOpenStudio')}</a>
            <button class="menu-item" role="menuitem" onclick={() => { close(); openCreate(link!.target); }}><Icon name="copy-plus" />{tr('detDuplicate')}</button>
            <button class="menu-item" role="menuitem" disabled={working}
              onclick={() => { close(); void organize({ favorite: !link!.favorite }, link!.favorite ? 'favoriteRemoved' : 'favoriteAdded'); }}>
              <Icon name="star" />{link?.favorite ? tr('detFavoriteRemove') : tr('detFavoriteAdd')}
            </button>
            <button class="menu-item" role="menuitem" disabled={working} onclick={() => { close(); void toggleEnabled(); }}>
              <Icon name={link?.enabled ? 'circle-pause' : 'play'} />{link?.enabled ? tr('detDisable') : tr('detEnable')}
            </button>
            <div class="menu-sep" role="separator"></div>
            <button class="menu-item" role="menuitem" disabled={working}
              onclick={() => { close(); void organize({ archived: !link!.archived }, link!.archived ? 'linkUnarchived' : 'linkArchived'); }}>
              <Icon name="archive" />{link?.archived ? tr('unarchiveLink') : tr('archiveLink')}
            </button>
            <button class="menu-item" role="menuitem" data-tone="danger" onclick={() => { close(); overlays.editId = link!.id; }}><Icon name="trash-2" />{tr('detDeleteEllipsis')}</button>
          {/snippet}
        </Menu>
      </div>
    </section>

    <div class="tabs" role="tablist" aria-label={tr('detSections')} tabindex="-1" onkeydown={tabKey}>
      {#each tabs as [key, label, count] (key)}
        <button id="{uid}-tab-{key}" type="button" class="tab" role="tab" aria-selected={tab === key} aria-controls="{uid}-panel"
          tabindex={tab === key ? 0 : -1} onclick={() => (tab = key)}>
          {label}{#if count}<span class="count num">{count}</span>{/if}
        </button>
      {/each}
    </div>

    <div id="{uid}-panel" role="tabpanel" aria-labelledby="{uid}-tab-{tab}" class="stack">
      {#if tab === 'overview'}
        <section class="panel kpis" aria-label={tr('detKeyFigures')}>
          <div class="kpi"><span>{tr('detVisits30')}</span><b class="num">{link.analytics_enabled ? num(link.visits_30d ?? summary?.totals.redirect) : tr('detOff')}</b></div>
          <div class="kpi"><span>{tr('detRecorded')}</span><b class="num">{num(link.recorded_redirects)}</b></div>
          <div class="kpi">
            <span>{tr('detLastVisit')}</span>
            <b>{link.last_redirect_at ? ago(link.last_redirect_at) : link.last_redirect_date ? day(link.last_redirect_date) : tr('never')}</b>
          </div>
          <div class="kpi"><span>{tr('detCreated')}</span><b>{date(link.created_at)}</b></div>
        </section>

        <section class="panel">
          <header class="panel-head">
            <div class="panel-title"><h2>{tr('detRouteTitle')}</h2><p>{tr('detRouteHint')}</p></div>
            <span class="spacer"></span>
            <button type="button" class="btn" data-size="sm" onclick={() => (tab = 'rules')}><Icon name="split" />{ruleCount ? tr('detEditRules') : tr('addRule')}</button>
          </header>
          <div class="panel-body">
            {#if policyError}<p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(policyError)}</p>
            {:else}
              <RouteMap host={link.host} slug={link.slug} target={link.target} {checks}
                rules={(policy?.rules ?? []).map((r) => ({ label: describeRule(r), target: r.target }))} />
            {/if}
          </div>
        </section>

        <div class="grid-main-side">
          <section class="panel">
            <header class="panel-head"><div class="panel-title"><h2>{tr('detVisitsTitle')}</h2><p>{tr('detVisitsHint')}</p></div></header>
            <div class="panel-body chart-body">
              {#if !link.analytics_enabled}
                <p class="muted">{tr('detAnalyticsOffBody')}</p>
              {:else if summaryError}
                <div class="stack">
                  <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(summaryError)}</p>
                  <div><button type="button" class="btn" data-size="sm" onclick={retrySummary}><Icon name="refresh-cw" />{tr('retry')}</button></div>
                </div>
              {:else if visits}
                <TimeChart labels={visits.labels} series={[{ name: tr('detVisitsTitle'), values: visits.values, color: 'var(--chart-1)', area: true }]}
                  height={200} caption={tr('detVisitsCaption', { slug: link.slug })} />
              {:else}
                <p class="muted" role="status">{tr('loading')}</p>
              {/if}
            </div>
          </section>
          <div class="stack">
            {#if app.previews}
              <section class="panel" aria-labelledby="{uid}-preview">
                <header class="panel-head">
                  <div class="panel-title"><h2 id="{uid}-preview">{tr('pvTitle')}</h2><p>{tr('pvHint')}</p></div>
                  <span class="spacer"></span>
                  {#if !link.protection}
                    <button type="button" class="btn" data-variant="ghost" data-size="sm" disabled={previewBusy} aria-busy={previewBusy} onclick={refreshPreview}><Icon name="refresh-cw" />{tr('pvRefresh')}</button>
                  {/if}
                </header>
                <div class="panel-body stack">
                  <LinkPreview id={link.id} preview={link.preview} size="card" />
                  {#if link.preview}
                    <dl class="facts">
                      <dt>{tr('pvKind')}</dt><dd>{link.preview.kind === 'icon' ? tr('pvKindIcon') : tr('pvKindImage')}</dd>
                      <dt>{tr('pvSource')}</dt><dd class="mono truncate">{link.preview.origin}</dd>
                      <dt>{tr('pvFetched')}</dt><dd>{when(link.preview.fetched_at)}</dd>
                    </dl>
                  {:else}
                    <p class="muted" role="status">{link.protection ? tr('pvProtected') : previewWaiting ? tr('pvPending') : tr('pvNone')}</p>
                  {/if}
                </div>
              </section>
            {/if}
            <section class="panel">
              <header class="panel-head">
                <div class="panel-title"><h2>{tr('detSettingsTitle')}</h2></div>
                <span class="spacer"></span>
                <button type="button" class="btn" data-variant="ghost" data-size="sm" onclick={() => (overlays.editId = link!.id)}>{tr('detEdit')}</button>
              </header>
              <dl class="facts panel-body">
                {#if policy}
                  <dt>{tr('detRedirect')}</dt><dd>{policy.status} · {policy.status === 301 || policy.status === 308 ? tr('permanent') : tr('temporary')}</dd>
                  <dt>{tr('detQuery')}</dt><dd>{policy.forward_query ? tr('detForwarded') : tr('detNotForwarded')}</dd>
                  <dt>{tr('detExtraPath')}</dt><dd>{policy.forward_path ? tr('detAllowed') : tr('detNotAllowed')}</dd>
                  <dt>{tr('detSearchEngines')}</dt><dd>{policy.noindex ? tr('detNoIndex') : tr('detIndexable')}</dd>
                {/if}
                <dt>{tr('detAnalytics')}</dt><dd>{link.analytics_enabled ? tr('detAnalyticsRecorded') : tr('detOff')}</dd>
                {#if link.monitor !== undefined}
                  <dt>{tr('detMonitoring')}</dt><dd>{link.monitor ? (link.monitor.priority ? tr('detMonitoredPriority') : tr('detMonitored')) : tr('detOff')}</dd>
                {/if}
                <dt>{tr('detCreated')}</dt><dd>{date(link.created_at)}</dd>
                <dt>{tr('detId')}</dt><dd class="mono">{link.id}</dd>
              </dl>
            </section>
          </div>
        </div>
      {:else if tab === 'analytics'}
        {#if !link.analytics_enabled}
          <div class="notice" data-tone="neutral"><Icon name="eye-off" /><div><b>{tr('detAnalyticsOffTitle')}</b><p>{tr('detAnalyticsOffBody')}</p></div></div>
        {:else}
          <AnalyticsTab linkId={link.id} slug={link.slug} {summary} error={summaryError} onretry={retrySummary} />
        {/if}
      {:else if tab === 'rules'}
        {#if policyError}
          <p class="notice" data-tone="error" role="alert"><Icon name="circle-alert" />{tr(policyError)}</p>
          <div><button type="button" class="btn" onclick={() => loadPolicy(link!.id, request)}><Icon name="refresh-cw" />{tr('retry')}</button></div>
        {:else if !policy}
          <p class="muted" role="status">{tr('loading')}</p>
        {:else}
          <RulesTab linkId={link.id} target={link.target} {policy} onsaved={onRulesSaved} />
        {/if}
      {:else if tab === 'access'}
        <div class="grid-2">
          <section class="panel">
            <header class="panel-head">
              <div class="panel-title"><h2>{tr('detScheduleTitle')}</h2><p>{tr('detScheduleHint', { zone: timeZone })}</p></div>
              <span class="spacer"></span>
              <button type="button" class="btn" data-variant="ghost" data-size="sm" onclick={() => (overlays.editId = link!.id)}>{tr('detEdit')}</button>
            </header>
            <dl class="facts panel-body">
              <dt>{tr('detStarts')}</dt><dd>{startsAt ? when(startsAt) : tr('detImmediately')}</dd>
              <dt>{tr('detEnds')}</dt><dd>{expiresAt ? when(expiresAt) : tr('detNoEnd')}</dd>
              <dt>{tr('status')}</dt><dd><Badge state={st} /></dd>
            </dl>
          </section>
          <section class="panel">
            <header class="panel-head"><div class="panel-title"><h2>{tr('detLimitTitle')}</h2><p>{tr('detLimitHint')}</p></div></header>
            <div class="panel-body limit">
              {#if maxVisits}
                <div class="limit-n"><b class="num">{num(visitsUsed)}</b><span class="muted num">{tr('detLimitOf', { max: num(maxVisits) })}</span></div>
                <div class="meter" role="meter" aria-label={tr('detLimitTitle')} aria-valuemin={0} aria-valuemax={maxVisits} aria-valuenow={Math.min(visitsUsed, maxVisits)}
                  data-tone={visitsUsed >= maxVisits ? 'error' : visitsUsed / maxVisits >= 0.8 ? 'warning' : 'accent'}>
                  <span style:width="{Math.min(100, (visitsUsed / maxVisits) * 100)}%"></span>
                </div>
                <p class="field-hint">{visitsUsed >= maxVisits ? tr('detLimitReached') : tr('detLimitStops')}</p>
              {:else}
                <p class="muted">{tr('detNoLimit')} <button type="button" class="btn" data-variant="link" onclick={() => (overlays.editId = link!.id)}>{tr('detSetLimit')}</button></p>
              {/if}
            </div>
          </section>
          <section class="panel">
            <header class="panel-head"><div class="panel-title"><h2>{tr('detProtectionTitle')}</h2></div></header>
            <div class="panel-body stack">
              <p class="secondary">
                {protection === 'password' ? tr('detProtectionPassword')
                  : protection === 'one_time' ? (consumed || link.state === 'exhausted' ? tr('oneTimeConsumed') : tr('detProtectionOneTime'))
                  : protection === null ? tr('detProtectionPublic') : tr('loading')}
              </p>
              <div class="btn-group">
                <button type="button" class="btn" onclick={() => (overlays.editId = link!.id)}>
                  <Icon name={protection ? 'key-round' : 'lock'} />{protection ? tr('detProtectionChange') : tr('detProtectionAdd')}
                </button>
              </div>
            </div>
          </section>
        </div>
      {:else if tab === 'health'}
        <HealthTab linkId={link.id} monitor={link.monitor} onchanged={linksChanged} />
      {:else}
        <HistoryTab {link} onrestored={linksChanged} oncount={(n) => (historyCount = n)} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .hero {
    display: flex;
    align-items: flex-start;
    gap: var(--space-6);
    padding: var(--space-6);
  }
  .hero-main {
    display: grid;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }
  .badges,
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .hero-url {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .short {
    display: inline-flex;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
  }
  .short .host {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 0 1 auto;
  }
  .short .slug {
    color: var(--accent);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
  }
  .hero-dest {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
    text-decoration: none;
  }
  .hero-dest b {
    color: var(--text-secondary);
    font-weight: var(--weight-regular);
  }
  .hero-dest :global(svg) {
    flex: none;
    width: 14px;
    height: 14px;
  }
  .hero-dest:hover {
    color: var(--text);
  }
  .description {
    font-size: var(--text-sm);
  }
  .hero-actions {
    flex-wrap: nowrap;
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
  .kpi {
    display: grid;
    gap: 4px;
    padding: var(--space-4) var(--space-5);
  }
  .kpi + .kpi {
    border-left: 1px solid var(--border-subtle);
  }
  .kpi span {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .kpi b {
    font-size: var(--text-xl);
    line-height: var(--leading-xl);
    font-weight: var(--weight-semibold);
  }
  .chart-body {
    padding-left: var(--space-3);
  }
  .limit {
    display: grid;
    gap: 10px;
  }
  .limit-n {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .limit-n b {
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
  }
  .limit .meter {
    height: 6px;
  }
  @media (max-width: 1000px) {
    .hero {
      flex-direction: column;
    }
    .hero-actions {
      flex-wrap: wrap;
    }
  }
  @media (max-width: 640px) {
    .hero {
      padding: var(--space-4);
    }
    .kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .kpi:nth-child(3) {
      border-left: 0;
    }
    .kpi:nth-child(n + 3) {
      border-top: 1px solid var(--border-subtle);
    }
    .short {
      flex-wrap: wrap;
      font-size: var(--text-lg);
    }
    .short .host {
      white-space: normal;
    }
    .hide-sm {
      display: none;
    }
  }
</style>
