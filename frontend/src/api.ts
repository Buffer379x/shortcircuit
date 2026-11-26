export interface User {
  username: string;
  csrf: string;
}

export interface Session {
  setup_required: boolean;
  user: User | null;
}

export interface Domains {
  hosts: string[];
  origins: Record<string, string>;
  local_port: number;
}

export interface Domain {
  host: string;
  origin: string;
  enabled: boolean;
  root_target: string | null;
  missing_target: string | null;
}

export interface Tag {
  name: string;
  link_count: number;
}

export type ConditionType = 'language' | 'device' | 'os' | 'browser' | 'query';

export interface RedirectCondition {
  type: ConditionType;
  key?: string;
  value: string;
}

export interface RedirectRule {
  target: string;
  conditions: RedirectCondition[];
}

export interface Policy {
  status: number;
  starts_at: number | null;
  expires_at: number | null;
  max_visits: number | null;
  forward_query: boolean;
  forward_path: boolean;
  rules: RedirectRule[];
  visits_used: number;
}

export interface Link {
  id: number;
  target: string;
  slug: string;
  host: string;
  title: string;
  enabled: boolean;
  analytics_enabled: boolean;
  tags: string[];
  created_at: string;
}

export interface LinkProtection {
  password_required: boolean;
  one_time: boolean;
  consumed: boolean;
}

export interface LinkProtectionChange {
  one_time: boolean;
  password?: string;
  remove_password?: boolean;
}

export type LinkMetadata = Pick<Link, 'target' | 'title' | 'tags' | 'enabled' | 'analytics_enabled'>;
export type LinkPolicy = Omit<Policy, 'visits_used'>;

export interface LinkRevision {
  id: number;
  created_at: number;
  actor: { kind: 'admin' | 'api_key'; label: string };
  action: 'metadata' | 'policy' | 'protection' | 'restore';
  previous: { kind: 'metadata'; value: LinkMetadata } | { kind: 'policy'; value: LinkPolicy } | null;
}

export interface LinkHistory {
  current: { metadata: LinkMetadata; policy: LinkPolicy };
  items: LinkRevision[];
}

export const linkSorts = [
  'newest', 'oldest', 'title_asc', 'title_desc', 'slug_asc', 'slug_desc',
  'host_asc', 'host_desc', 'target_asc', 'target_desc', 'enabled_first', 'disabled_first',
] as const;
export type LinkSort = typeof linkSorts[number];

export interface LinkCursor {
  id: number;
  key: string;
  sort: LinkSort;
}

export interface LinkSearch {
  query?: string;
  host?: string;
  tag?: string;
  enabled?: boolean;
  sort: LinkSort;
  after?: LinkCursor;
}

export interface LinkSearchPage {
  items: CatalogLink[];
  next_cursor: LinkCursor | null;
}

export interface CatalogLink extends Link {
  selection_key: string;
}

export interface BulkLinkResult {
  items: Link[];
  deleted_ids: number[];
}

export interface QrStyle {
  size: number;
  foreground: string;
  background: string;
  transparent: boolean;
  correction: 'L' | 'M' | 'Q' | 'H';
  quiet_zone: number;
  frame: 'none' | 'outline';
}

export interface QrTemplate extends QrStyle {
  id: number;
  name: string;
}

export const scopes = ['read', 'write', 'delete'] as const;
export type Scope = typeof scopes[number];

export interface Key {
  id: number;
  name: string;
  scopes: string[];
  host: string | null;
  expires_at: number | null;
  created_at: number;
}

export class ApiError extends Error {
  constructor(readonly status: number, readonly code: string) {
    super(code);
  }
}

const responseTypes = { json: 'application/json', csv: 'text/csv', image: 'image/png, image/svg+xml' };
type ResponseFormat = keyof typeof responseTypes;

export interface Api {
  <T>(path: string, method?: 'GET' | 'POST' | 'PATCH' | 'DELETE', data?: unknown, responseFormat?: ResponseFormat, signal?: AbortSignal): Promise<T>;
}

export function createApi(csrf: () => string | undefined, request = fetch): Api {
  return async function<T>(path: string, method = 'GET', data?: unknown, responseFormat: ResponseFormat = 'json', signal?: AbortSignal): Promise<T> {
    const headers = new Headers({ Accept: responseTypes[responseFormat] });
    const token = csrf();
    if (method !== 'GET' && token) headers.set('X-CSRF-Token', token);
    const body = data instanceof Blob ? data : data === undefined ? undefined : JSON.stringify(data);
    if (body !== undefined) headers.set('Content-Type', data instanceof Blob ? data.type : 'application/json');
    const response = await request(`/api/v1${path}`, {
      method,
      credentials: 'same-origin',
      headers,
      body,
      signal,
    });
    if (!response.ok) {
      const problem = await response.json().catch(() => null);
      const code = typeof problem?.code === 'string' ? problem.code : '';
      throw new ApiError(response.status, code);
    }
    if (response.status === 204) return undefined as T;
    return responseFormat === 'json' ? response.json() : response as T;
  };
}

export function shortUrl(link: Pick<Link, 'host' | 'slug'>, domains: Domains): string {
  const configured = Object.hasOwn(domains.origins, link.host) ? domains.origins[link.host] : undefined;
  return `${configured ?? `https://${link.host}`}/${link.slug}`;
}

export function linkName(link: Link): string {
  return link.title || `/${link.slug}`;
}
