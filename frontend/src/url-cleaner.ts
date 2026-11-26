const defaultTrackers = new Set([
  'utm_source', 'utm_medium', 'utm_campaign', 'utm_content', 'utm_term',
  'fbclid', 'gclid', 'dclid', 'msclkid',
]);

const protectedKeys = new Set([
  'sig', 'signature', 'token', 'access_token', 'auth', 'authorization',
  'key-pair-id', 'policy', 'hmac', 'jwt', 'session', 'sessionid', 'expires',
]);

export type CleanUrlProposal =
  | { status: 'invalid' | 'protected' | 'unchanged' }
  | { status: 'changes'; cleaned: string; removed: string[] };

function parameterKey(field: string): string {
  const equals = field.indexOf('=');
  const raw = equals === -1 ? field : field.slice(0, equals);
  try {
    return decodeURIComponent(raw.replaceAll('+', ' ')).toLowerCase();
  } catch {
    return '';
  }
}

export function proposeCleanUrl(
  original: string,
  removable: ReadonlySet<string> = defaultTrackers,
  exceptions: ReadonlySet<string> = new Set(),
): CleanUrlProposal {
  if (!/^https?:\/\//i.test(original) || /[\s\\\p{Cc}]|%(?![\da-f]{2})/iu.test(original)) {
    return { status: 'invalid' };
  }
  let url: URL;
  try {
    url = new URL(original);
  } catch {
    return { status: 'invalid' };
  }
  const authority = original.match(/^https?:\/\/([^/?#]+)/i)?.[1];
  if (!authority || authority.includes('@') || url.username || url.password) {
    return { status: 'invalid' };
  }

  const hash = original.indexOf('#');
  const base = hash === -1 ? original : original.slice(0, hash);
  const question = base.indexOf('?');
  if (question === -1) return { status: 'unchanged' };

  const fields = base.slice(question + 1).split('&').map((raw) => ({ raw, key: parameterKey(raw) }));
  const protectedQuery = fields.some(({ key }) => (
    protectedKeys.has(key) || key.startsWith('x-amz-') || key.startsWith('x-goog-')
  ));
  if (protectedQuery || exceptions.has(url.hostname.toLowerCase())) return { status: 'protected' };

  const kept: string[] = [];
  const removed = new Set<string>();
  for (const { raw, key } of fields) {
    if (removable.has(key)) removed.add(key);
    else kept.push(raw);
  }
  if (!removed.size) return { status: 'unchanged' };

  const fragment = hash === -1 ? '' : original.slice(hash);
  const query = kept.length ? '?' + kept.join('&') : '';
  return { status: 'changes', cleaned: base.slice(0, question) + query + fragment, removed: [...removed] };
}
