// Report shapes and helpers shared by the Overview and Analytics pages. The API returns UTC day
// numbers (days since the Unix epoch); the charts take ISO dates.
import type { Api } from '../api';
import type { Messages } from '../i18n';
import { locale, tr } from '../lib/locale.svelte';

export const DAY_MS = 86_400_000;

const categories = {
  android: 'analyticsAndroid', automated: 'analyticsAutomated', chrome: 'analyticsChrome', chromeos: 'analyticsChromeOs',
  desktop: 'analyticsDesktop', direct: 'analyticsDirect', edge: 'analyticsEdge', firefox: 'analyticsFirefox',
  internal: 'analyticsInternal', ios: 'analyticsIos', linux: 'analyticsLinux', macos: 'analyticsMacos',
  mobile: 'analyticsMobile', opera: 'analyticsOpera', preview: 'analyticsPreview', referral: 'analyticsReferral',
  safari: 'analyticsSafari', tablet: 'analyticsTablet', unknown: 'analyticsUnknownCategory', windows: 'analyticsWindows',
} as const satisfies Record<string, keyof Messages>;

export type Category = keyof typeof categories;
export type Dimension = 'traffic_source' | 'browser' | 'os' | 'device';
export interface RankedLink { id: number; slug: string; host: string; title: string; total: number }

export interface Summary {
  days: number;
  today: number;
  start_day: number;
  end_day: number;
  host: string | null;
  totals: Record<'redirect' | 'blocked' | 'missing' | 'root_fallback' | 'root_missing' | 'human' | 'bot' | 'unknown', number>;
  previous_redirects: number;
  daily: { day: number; redirect: number }[];
  top_links: RankedLink[];
  top_domains: { host: string; total: number }[];
  top_tags: { tag: string; total: number }[];
  breakdowns: Record<Dimension, { category: Category; total: number }[]>;
  dropped: number | null;
}

export type Period = { days: number } | { start: string; end: string };

/** A category label from the catalog; unknown categories fall back to "Unknown". */
export function categoryLabel(category: string) {
  return tr(categories[category as Category] ?? 'analyticsUnknownCategory');
}

export function isoDay(day: number) {
  return new Date(day * DAY_MS).toISOString().slice(0, 10);
}

/** "30 days" in the active language. */
export function dayCount(days: number) {
  return new Intl.NumberFormat(locale.language, { style: 'unit', unit: 'day', unitDisplay: 'long' }).format(days);
}

/** A share such as "90 %", without the sign that `fmt.percent` adds for changes. */
export function share(part: number, total: number) {
  return total > 0
    ? new Intl.NumberFormat(locale.language, { style: 'percent', maximumFractionDigits: 0 }).format(part / total)
    : '—';
}

export function summaryQuery(period: Period, host: string | null) {
  const query = new URLSearchParams();
  if ('days' in period) query.set('days', String(period.days));
  else {
    query.set('start', period.start);
    query.set('end', period.end);
  }
  if (host) query.set('host', host);
  return query;
}

/** Daily redirects for the period of equal length before `report`, or null when it would start
 * before the epoch or cannot be loaded. The comparison line is optional, so errors stay silent. */
export async function previousDaily(api: Api, report: Summary, signal: AbortSignal) {
  const length = report.end_day - report.start_day + 1;
  const start = report.start_day - length;
  if (start < 0) return null;
  const query = summaryQuery({ start: isoDay(start), end: isoDay(report.start_day - 1) }, report.host);
  try {
    const previous = await api<Summary>(`/analytics/summary?${query}`, 'GET', undefined, 'json', signal);
    return previous.daily.map((point) => point.redirect);
  } catch {
    return null;
  }
}

/** Donut items: the four largest categories and the rest summed as "Other", so shares stay true. */
export function donutItems(rows: { label: string; total: number }[]) {
  const sorted = [...rows].filter((row) => row.total > 0).sort((a, b) => b.total - a.total);
  const items = sorted.slice(0, sorted.length > 5 ? 4 : 5).map((row) => ({ label: row.label, visits: row.total }));
  const rest = sorted.slice(items.length).reduce((sum, row) => sum + row.total, 0);
  if (rest > 0) items.push({ label: tr('insOther'), visits: rest });
  return items;
}

/** The donut's center label, only when it fits the ring. */
export function donutCenter(items: { label: string }[]) {
  const label = items[0]?.label ?? '';
  return label.length <= 12 ? label : undefined;
}

/** Spreadsheet cells that start like a formula are prefixed with an apostrophe, as the link CSV
 * export does, so a report never runs code when opened in a spreadsheet. */
function cell(value: string | number | null) {
  let text = value == null ? '' : String(value);
  if (typeof value === 'string' && /^\s*['=+\-@＝＋－＠]/.test(text)) text = `'${text}`;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** The versioned JSON export as a long-format CSV: one row per figure. */
export function reportCsv(exported: { version: number; granularity: string; report: Summary }) {
  const { report } = exported;
  const rows: (string | number | null)[][] = [['version', 'section', 'key', 'value']];
  const add = (section: string, key: string | number | null, value: string | number | null) =>
    rows.push([exported.version, section, key, value]);
  add('period', 'start', isoDay(report.start_day));
  add('period', 'end', isoDay(report.end_day));
  add('period', 'granularity', exported.granularity);
  add('period', 'host', report.host);
  for (const [key, value] of Object.entries(report.totals)) add('totals', key, value);
  add('totals', 'previous_redirects', report.previous_redirects);
  if (report.dropped != null) add('totals', 'dropped', report.dropped);
  for (const point of report.daily) add('daily', isoDay(point.day), point.redirect);
  for (const link of report.top_links) add('top_links', `${link.host}/${link.slug}`, link.total);
  for (const domain of report.top_domains) add('top_domains', domain.host, domain.total);
  for (const tag of report.top_tags) add('top_tags', tag.tag, tag.total);
  for (const [dimension, items] of Object.entries(report.breakdowns)) {
    for (const item of items) add(dimension, item.category, item.total);
  }
  return rows.map((row) => row.map(cell).join(',')).join('\r\n') + '\r\n';
}
