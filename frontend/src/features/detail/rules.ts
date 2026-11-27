// Redirect rule vocabulary shared by the rule editor, the route map, and the history diff.
// The server compares every condition for equality; the first rule whose conditions all match wins.
import type { ConditionType, RedirectCondition, RedirectRule } from '../../api';
import type { Messages } from '../../i18n';
import { tr } from '../../lib/locale.svelte';

/** Limits enforced by the server (src/redirect.rs). */
export const MAX_RULES = 16;
export const MAX_CONDITIONS = 8;

export const conditionLabels = {
  language: 'matchLanguage',
  device: 'matchDevice',
  os: 'matchOs',
  browser: 'matchBrowser',
  query: 'matchQuery',
} as const satisfies Record<ConditionType, keyof Messages>;

/** Values the server classifies from the User-Agent header. */
export const conditionOptions: Partial<Record<ConditionType, string[]>> = {
  device: ['desktop', 'mobile', 'tablet', 'unknown'],
  os: ['windows', 'macos', 'linux', 'android', 'ios', 'chromeos', 'unknown'],
  browser: ['edge', 'firefox', 'chrome', 'safari', 'opera', 'unknown'],
};

const valueLabels: Record<string, keyof Messages> = {
  desktop: 'analyticsDesktop', mobile: 'analyticsMobile', tablet: 'analyticsTablet',
  windows: 'analyticsWindows', macos: 'analyticsMacos', linux: 'analyticsLinux', android: 'analyticsAndroid',
  ios: 'analyticsIos', chromeos: 'analyticsChromeOs', edge: 'analyticsEdge', firefox: 'analyticsFirefox',
  chrome: 'analyticsChrome', safari: 'analyticsSafari', opera: 'analyticsOpera', unknown: 'unknown',
};

export function optionLabel(value: string) {
  return valueLabels[value] ? tr(valueLabels[value]) : value;
}

export function newCondition(type: ConditionType = 'query'): RedirectCondition {
  return type === 'query' ? { type, key: '', value: '' } : { type, value: conditionOptions[type]?.[0] ?? '' };
}

export function describeCondition(c: RedirectCondition) {
  if (c.type === 'query') return tr('detRuleQueryIs', { key: c.key ?? '', value: c.value || tr('detRuleEmpty') });
  return tr('detRuleIs', { field: tr(conditionLabels[c.type]), value: optionLabel(c.value) });
}

export function describeRule(rule: RedirectRule) {
  return rule.conditions.map(describeCondition).join(tr('detRuleAnd'));
}

/** Server-side language tag check: segments of 1 to 8 ASCII letters or digits, 63 bytes at most. */
export function validLanguage(tag: string) {
  return tag.length > 0 && tag.length <= 63 && tag.split('-').every((s) => /^[A-Za-z0-9]{1,8}$/.test(s));
}

export function validTarget(value: string) {
  try {
    const url = new URL(value);
    return (url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password;
  } catch {
    return false;
  }
}

// eslint-disable-next-line no-control-regex
const CONTROL = /\p{Cc}/u;

export type RuleErrors = Record<string, keyof Messages>;

/** Validates the draft like the server does. Keys: `<rule>-target`, `<rule>-<condition>-key|value`. */
export function validateRules(rules: RedirectRule[]): RuleErrors {
  const errors: RuleErrors = {};
  rules.forEach((rule, r) => {
    rule.conditions.forEach((c, i) => {
      if (c.type === 'query') {
        const key = c.key ?? '';
        if (!key || key.length > 128 || CONTROL.test(key)) errors[`${r}-${i}-key`] = 'detRuleKeyInvalid';
        if (c.value.length > 512 || CONTROL.test(c.value)) errors[`${r}-${i}-value`] = 'detRuleValueInvalid';
      } else if (c.type === 'language' && !validLanguage(c.value.trim())) {
        errors[`${r}-${i}-value`] = 'detRuleLanguageInvalid';
      }
    });
    if (!validTarget(rule.target.trim())) errors[`${r}-target`] = 'invalidTarget';
  });
  return errors;
}

/** A copy without shared references, trimmed the way the server stores it. */
export function cleanRules(rules: RedirectRule[]): RedirectRule[] {
  return rules.map((rule) => ({
    target: rule.target.trim(),
    conditions: rule.conditions.map((c) =>
      c.type === 'query' ? { type: c.type, key: c.key ?? '', value: c.value } : { type: c.type, value: c.type === 'language' ? c.value.trim() : c.value }),
  }));
}
