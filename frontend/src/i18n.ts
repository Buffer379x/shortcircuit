// The central English and German catalog. Each area keeps its strings in i18n/<area>.ts;
// this module merges them into one typed catalog. Keys must be unique across files.
import * as core from './i18n/core';
import * as legacy from './i18n/legacy';
import * as links from './i18n/links';
import * as detail from './i18n/detail';
import * as insights from './i18n/insights';
import * as admin from './i18n/admin';

const en = { ...legacy.en, ...core.en, ...links.en, ...detail.en, ...insights.en, ...admin.en };
const de: Record<keyof typeof en, string> = { ...legacy.de, ...core.de, ...links.de, ...detail.de, ...insights.de, ...admin.de };

export type Messages = Record<keyof typeof en, string>;
export type Language = 'en' | 'de';
export type Theme = 'dark' | 'light' | 'system';
export const messages = { en, de };

/** Replaces {name} placeholders. */
export function fill(text: string, values: Record<string, string | number>) {
  return text.replace(/\{(\w+)\}/g, (_, key: string) => String(values[key] ?? `{${key}}`));
}
