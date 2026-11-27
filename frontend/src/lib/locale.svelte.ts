// The active language and catalog. App.svelte sets the language; components read `locale.t`.
import { messages, fill, type Language, type Messages } from '../i18n';

export const locale = $state({ language: 'en' as Language, t: messages.en as Messages });

export function setLanguage(language: Language) {
  locale.language = language;
  locale.t = messages[language];
  document.documentElement.lang = language;
  try {
    localStorage.setItem('shortcircuit-language', language);
  } catch {
    /* The choice still applies to this page when storage is unavailable. */
  }
}

/** Translates a key and fills {placeholders}. */
export function tr(key: keyof Messages, values?: Record<string, string | number>) {
  const text = locale.t[key];
  return values ? fill(text, values) : text;
}
