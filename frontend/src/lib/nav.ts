import type { Messages } from '../i18n';
import type { IconName } from './icons';
import { tr } from './locale.svelte';

export interface NavItem { page: string; label: keyof Messages; icon: IconName }
export const NAV: { label?: keyof Messages; items: NavItem[] }[] = [
  {
    items: [
      { page: 'overview', label: 'navOverview', icon: 'layout-dashboard' },
      { page: 'links', label: 'navLinks', icon: 'link' },
      { page: 'analytics', label: 'navAnalytics', icon: 'chart-column' },
      { page: 'health', label: 'navHealth', icon: 'activity' },
      { page: 'qr', label: 'navQr', icon: 'qr-code' },
    ],
  },
  {
    label: 'navGroupConfiguration',
    items: [
      { page: 'domains', label: 'navDomains', icon: 'globe' },
      { page: 'exchange', label: 'navExchange', icon: 'arrow-left-right' },
      { page: 'keys', label: 'navKeys', icon: 'key-round' },
    ],
  },
  {
    label: 'navGroupSystem',
    items: [
      { page: 'system', label: 'navSystem', icon: 'server' },
      { page: 'settings', label: 'navSettings', icon: 'settings' },
    ],
  },
];

export function pageTitle(page: string) {
  if (page === 'tags') return tr('navTags');
  const item = NAV.flatMap((g) => g.items).find((i) => i.page === page);
  return item ? tr(item.label) : 'ShortCircuit';
}
