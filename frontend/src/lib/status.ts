// The one mapping from link and health states to tone, icon, and text. Every view uses it,
// so a state always looks and reads the same and never relies on color alone.
import type { CatalogLink, MonitorStatus } from '../api';
import type { IconName } from './icons';
import { tr } from './locale.svelte';

export type Tone = 'success' | 'warning' | 'error' | 'info' | 'muted' | 'neutral' | 'accent';
export interface StateView { tone: Tone; icon: IconName; label: string; hint?: string }
export type Health = 'up' | 'redirect' | 'warning' | 'down' | 'pending' | 'unchecked';

export function linkState(l: Pick<CatalogLink, 'archived' | 'state' | 'expires_at'>): StateView {
  if (l.archived) return { tone: 'muted', icon: 'archive', label: tr('stateArchived') };
  if (l.state === 'disabled') return { tone: 'muted', icon: 'circle-pause', label: tr('stateDisabled') };
  if (l.state === 'scheduled') return { tone: 'info', icon: 'calendar-clock', label: tr('stateScheduled') };
  if (l.state === 'expired') return { tone: 'neutral', icon: 'hourglass', label: tr('stateExpired') };
  if (l.state === 'exhausted') return { tone: 'warning', icon: 'ban', label: tr('stateExhausted') };
  if (l.expires_at) {
    const days = Math.ceil((l.expires_at * 1000 - Date.now()) / 86_400_000);
    if (days <= 14) return { tone: 'warning', icon: 'clock', label: tr('stateExpiresIn', { days }), hint: tr('stateExpiresHint') };
  }
  return { tone: 'success', icon: 'circle-check', label: tr('stateActive') };
}

export function health(status: MonitorStatus | null | undefined): Health {
  switch (status) {
    case undefined:
    case null:
      return 'unchecked';
    case 'healthy':
      return 'up';
    case 'redirected':
      return 'redirect';
    case 'warning':
      return 'warning';
    case 'broken':
    case 'unreachable':
      return 'down';
    default:
      return 'pending';
  }
}

export function healthState(h: Health): StateView {
  switch (h) {
    case 'up': return { tone: 'success', icon: 'circle-check', label: tr('healthUp') };
    case 'redirect': return { tone: 'warning', icon: 'corner-down-right', label: tr('healthRedirect'), hint: tr('healthRedirectHint') };
    case 'warning': return { tone: 'warning', icon: 'triangle-alert', label: tr('monitorWarning') };
    case 'down': return { tone: 'error', icon: 'circle-x', label: tr('healthDown') };
    case 'pending': return { tone: 'muted', icon: 'clock', label: tr('healthPending') };
    default: return { tone: 'muted', icon: 'circle-dot', label: tr('healthUnchecked') };
  }
}
