import { ApiError } from './api';
import type { Language, Messages } from './i18n';

type Message = keyof Messages;

const problemMessages = new Map<string, Message>([
  ['invalid_credentials', 'invalidCredentials'],
  ['invalid_token', 'invalidToken'],
  ['invalid_password', 'invalidPassword'],
  ['invalid_username', 'invalidUsername'],
  ['invalid_origin', 'invalidOrigin'],
  ['invalid_fallback', 'invalidFallback'],
  ['fallback_loop', 'fallbackLoop'],
  ['domain_limit', 'domainLimit'],
  ['invalid_redirect_status', 'invalidRedirectStatus'],
  ['invalid_redirect_window', 'invalidRedirectWindow'],
  ['invalid_visit_limit', 'invalidVisitLimit'],
  ['permanent_redirect_requires_unrestricted_link', 'permanentRestriction'],
  ['protection_requires_temporary_redirect', 'protectionRequiresTemporary'],
  ['one_time_consumed', 'oneTimeConsumed'],
  ['invalid_redirect_rule', 'invalidRedirectRule'],
  ['invalid_target', 'invalidTarget'],
  ['invalid_slug', 'invalidSlug'],
  ['slug_in_use', 'slugInUse'],
  ['invalid_host', 'invalidHost'],
  ['invalid_title', 'invalidTitle'],
  ['invalid_tags', 'invalidTags'],
  ['invalid_link_search', 'invalidLinkSearch'],
  ['invalid_link_cursor', 'invalidLinkCursor'],
  ['invalid_scopes', 'invalidScopes'],
  ['invalid_expiration', 'invalidExpiration'],
  ['invalid_qr_color', 'invalidQrColor'],
  ['invalid_qr_contrast', 'invalidQrContrast'],
  ['invalid_qr_correction', 'invalidQrCorrection'],
  ['invalid_qr_quiet_zone', 'invalidQrQuietZone'],
  ['invalid_qr_frame', 'invalidQrFrame'],
  ['invalid_qr_logo_type', 'invalidQrLogoType'],
  ['invalid_qr_logo_body', 'invalidQrLogoBody'],
  ['invalid_qr_logo', 'invalidQrLogo'],
  ['invalid_qr_logo_dimensions', 'invalidQrLogoDimensions'],
  ['invalid_qr_logo_correction', 'invalidQrLogoCorrection'],
  ['qr_logo_timeout', 'qrLogoTimeout'],
  ['qr_logo_unreadable', 'qrLogoUnreadable'],
  ['invalid_qr_size', 'invalidQrSize'],
  ['invalid_qr_template_name', 'invalidQrTemplateName'],
  ['qr_template_exists', 'qrTemplateExists'],
  ['qr_template_limit', 'qrTemplateLimit'],
  ['not_found', 'resourceMissing'],
]);

export function problemMessage(error: unknown, unauthorized: Message = 'sessionExpired'): Message {
  if (!(error instanceof ApiError)) return 'requestFailed';
  if (error.status === 401) return unauthorized;
  const message = problemMessages.get(error.code);
  if (message) return message;
  if (error.status === 403) return 'accessDenied';
  if (error.status === 429) return 'tooManyRequests';
  if (error.status >= 500) return 'serviceUnavailable';
  return 'requestRejected';
}

export function passwordAllowed(value: string): boolean {
  return [...value].length >= 12 && new TextEncoder().encode(value).length <= 1024;
}

export function labelAllowed(value: string): boolean {
  return value.length > 0 && [...value].length <= 64 && !/\p{Cc}/u.test(value);
}

export function formatDate(date: string, language: Language, withTime = false): string {
  const parsed = new Date(date);
  if (Number.isNaN(parsed.getTime())) return date;
  return new Intl.DateTimeFormat(language, {
    dateStyle: 'medium',
    ...(withTime ? { timeStyle: 'short' } : {}),
  }).format(parsed);
}

export function fromUnixSeconds(seconds: number): string | null {
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}
