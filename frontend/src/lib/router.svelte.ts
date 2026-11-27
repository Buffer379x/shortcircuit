// Hash routing: #/<page>[/<id>][?key=value]. Reload, Back, and bookmarks restore the view.
// Hashes from before Polar Signal (#links, #operations, …) are mapped to the new pages.
export const route = $state({ page: 'overview', id: '', params: new URLSearchParams() });

const LEGACY: Record<string, string> = {
  overview: 'overview', links: 'links', analytics: 'analytics', monitoring: 'health', domains: 'domains',
  tags: 'tags', exchange: 'exchange', apiKeys: 'keys', account: 'settings', operations: 'system',
  auditLog: 'system?tab=audit',
};

function parse() {
  let raw = location.hash.replace(/^#/, '');
  if (raw && !raw.startsWith('/') && LEGACY[raw]) {
    history.replaceState(null, '', '#/' + LEGACY[raw]);
    raw = '/' + LEGACY[raw];
  }
  raw = raw.replace(/^\/?/, '');
  const [path, query = ''] = raw.split('?');
  const [page, ...rest] = path.split('/').filter(Boolean);
  route.page = page || 'overview';
  route.id = rest.join('/');
  route.params = new URLSearchParams(query);
}

parse();
addEventListener('hashchange', parse);

export function href(page: string, id?: string | number, params?: Record<string, string>) {
  const q = params ? '?' + new URLSearchParams(params).toString() : '';
  return `#/${page}${id != null && id !== '' ? '/' + id : ''}${q}`;
}

export function go(page: string, id?: string | number, params?: Record<string, string>) {
  location.hash = href(page, id, params);
}

/** Updates query parameters in place without adding history entries. Reads the URL, not reactive
 * state, so effects that sync filters into the URL do not depend on what they write. */
export function setParams(values: Record<string, string | null>) {
  const raw = location.hash.replace(/^#\/?/, '');
  const [path, query = ''] = raw.split('?');
  const p = new URLSearchParams(query);
  for (const [k, v] of Object.entries(values)) {
    if (v == null || v === '') p.delete(k);
    else p.set(k, v);
  }
  const q = p.toString();
  history.replaceState(null, '', `#/${path}${q ? '?' + q : ''}`);
}
