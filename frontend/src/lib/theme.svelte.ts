// Theme choice (dark, light, or system), stored with the key used before Polar Signal.
export type ThemeChoice = 'dark' | 'light' | 'system';
const media = matchMedia('(prefers-color-scheme: light)');
const KEY = 'shortcircuit-theme';

function stored(): ThemeChoice {
  try {
    const value = localStorage.getItem(KEY);
    if (value === 'dark' || value === 'light' || value === 'system') return value;
  } catch {
    /* Use the default. */
  }
  return 'system';
}

export const theme = $state({ choice: stored(), resolved: 'dark' as 'dark' | 'light' });

function apply() {
  theme.resolved = theme.choice === 'system' ? (media.matches ? 'light' : 'dark') : theme.choice;
  document.documentElement.dataset.theme = theme.resolved;
  // design-lint: allow, the browser chrome color must be a literal and matches --bg.
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme.resolved === 'dark' ? '#101115' : '#f5f5f8'); // design-lint: allow
}

export function setTheme(choice: ThemeChoice) {
  theme.choice = choice;
  try {
    localStorage.setItem(KEY, choice);
  } catch {
    /* The choice still applies to this page. */
  }
  apply();
}

media.addEventListener('change', apply);
apply();
