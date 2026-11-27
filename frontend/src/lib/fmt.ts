// Locale-aware formatting for the Polar Signal components.
import { locale, tr } from './locale.svelte';

const tz = () => { try { return localStorage.getItem('shortcircuit-timezone') || undefined; } catch { return undefined; } };

export const num = (n: number | null | undefined) => (n == null ? '—' : new Intl.NumberFormat(locale.language).format(n));
export const percent = (n: number) =>
  new Intl.NumberFormat(locale.language, { style: 'percent', maximumFractionDigits: 1, signDisplay: 'exceptZero' }).format(n);
/** A UTC day (YYYY-MM-DD or unix day number) as a short date. */
export function day(value: string | number) {
  const d = typeof value === 'number' ? new Date(value * 86_400_000) : new Date(value + (value.length === 10 ? 'T00:00:00Z' : ''));
  return new Intl.DateTimeFormat(locale.language, { day: 'numeric', month: 'short', timeZone: 'UTC' }).format(d);
}
/** Unix seconds or ISO text as a date in the display time zone. */
export function date(value: number | string) {
  const d = typeof value === 'number' ? new Date(value * 1000) : new Date(value);
  return new Intl.DateTimeFormat(locale.language, { dateStyle: 'medium', timeZone: tz() }).format(d);
}
export function when(value: number | string) {
  const d = typeof value === 'number' ? new Date(value * 1000) : new Date(value);
  return new Intl.DateTimeFormat(locale.language, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', timeZone: tz() }).format(d);
}
/** Relative time for unix seconds or ISO text. */
export function ago(value: number | string | null | undefined) {
  if (value == null) return tr('never');
  const ms = typeof value === 'number' ? value * 1000 : Date.parse(value);
  const s = (ms - Date.now()) / 1000;
  const abs = Math.abs(s);
  const rtf = new Intl.RelativeTimeFormat(locale.language, { numeric: 'auto' });
  if (abs < 60) return tr('justNow');
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
  if (abs < 86400 * 30) return rtf.format(Math.round(s / 86400), 'day');
  return date(value);
}
export function change(current: number, previous: number) {
  if (!previous) return { value: 0, dir: 'flat' as const };
  const value = (current - previous) / previous;
  return { value, dir: Math.abs(value) < 0.005 ? ('flat' as const) : value > 0 ? ('up' as const) : ('down' as const) };
}
export function splitUrl(url: string) {
  try {
    const u = new URL(url);
    return { host: u.host, rest: u.pathname + u.search + u.hash };
  } catch {
    return { host: url, rest: '' };
  }
}
