// Per-link analytics: GET /analytics/summary?link=<id> returns the instance summary restricted to one link.
import type { Messages } from '../../i18n';

export type Dimension = 'traffic_source' | 'browser' | 'os' | 'device';

export interface Summary {
  days: number;
  today: number;
  start_day: number;
  end_day: number;
  totals: Record<'redirect' | 'blocked' | 'missing' | 'root_fallback' | 'root_missing' | 'human' | 'bot' | 'unknown', number>;
  previous_redirects: number;
  daily: { day: number; redirect: number }[];
  breakdowns: Record<Dimension, { category: string; total: number }[]>;
  dropped: number | null;
}

export const categories: Record<string, keyof Messages> = {
  android: 'analyticsAndroid', automated: 'analyticsAutomated', chrome: 'analyticsChrome', chromeos: 'analyticsChromeOs',
  desktop: 'analyticsDesktop', direct: 'analyticsDirect', edge: 'analyticsEdge', firefox: 'analyticsFirefox',
  internal: 'analyticsInternal', ios: 'analyticsIos', linux: 'analyticsLinux', macos: 'analyticsMacos',
  mobile: 'analyticsMobile', opera: 'analyticsOpera', preview: 'analyticsPreview', referral: 'analyticsReferral',
  safari: 'analyticsSafari', tablet: 'analyticsTablet', unknown: 'analyticsUnknownCategory', windows: 'analyticsWindows',
};

/** A unix day number as YYYY-MM-DD. */
export const isoDay = (day: number) => new Date(day * 86_400_000).toISOString().slice(0, 10);

/** Daily redirects for every day of the period, including days without visits. */
export function series(summary: Summary) {
  const byDay = new Map(summary.daily.map((d) => [d.day, d.redirect]));
  const labels: string[] = [];
  const values: number[] = [];
  for (let d = summary.start_day; d <= summary.end_day; d++) {
    labels.push(isoDay(d));
    values.push(byDay.get(d) ?? 0);
  }
  return { labels, values };
}
