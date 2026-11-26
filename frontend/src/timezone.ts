import { getContext, setContext } from 'svelte';

const timezoneContext = Symbol('timezone');

export function defaultTimeZone(): string {
  return new Intl.DateTimeFormat().resolvedOptions().timeZone;
}

export function isTimeZone(value: string): boolean {
  try {
    new Intl.DateTimeFormat('en', { timeZone: value });
    return true;
  } catch {
    return false;
  }
}

export function provideTimeZone(read: () => string | undefined) {
  setContext(timezoneContext, read);
}

export function useTimeZone() {
  return getContext<(() => string | undefined) | undefined>(timezoneContext) ?? (() => undefined);
}
