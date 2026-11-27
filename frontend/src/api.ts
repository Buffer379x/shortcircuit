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
  block_crawlers: boolean;
}

export interface Tag {
  name: string;
  link_count: number;
  color?: string;
  description?: string;
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
  noindex?: boolean;
  rules: RedirectRule[];
  visits_used: number;
}

export interface SimulationResult {
  target: string;
  status: number;
  rule: number | null;
}

export interface Link {
  id: number;
  target: string;
  slug: string;
  host: string;
  title: string;
  description: string;
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

export type LinkMetadata = Pick<Link, 'target' | 'title' | 'tags' | 'enabled' | 'analytics_enabled'>
  & Partial<Pick<Link, 'description'>>;
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
  'visits_desc', 'visits_asc', 'last_visit_desc', 'last_visit_asc',
] as const;
export type LinkSort = typeof linkSorts[number];
export type LinkView = 'unarchived' | 'favorites' | 'archived';
export type TagMatch = 'all' | 'any';

export interface LinkCursor {
  id: number;
  key: string;
  sort: LinkSort;
}

export interface LinkSearch {
  query?: string;
  host?: string;
  tags?: string[];
  tag_match?: TagMatch;
  view: LinkView;
  enabled?: boolean;
  state?: LinkState;
  sort: LinkSort;
  after?: LinkCursor;
}

export interface LinkSearchPage {
  items: CatalogLink[];
  next_cursor: LinkCursor | null;
}

export interface LinkOrganization {
  favorite: boolean;
  archived: boolean;
}

export type LinkState = 'active' | 'scheduled' | 'expired' | 'disabled' | 'exhausted';
export type MonitorStatus = 'not_checked' | 'healthy' | 'redirected' | 'warning' | 'unreachable' | 'broken' | 'unknown';

/** Health check state of a link, or null when the link is not monitored. */
export interface LinkMonitorSummary {
  status: MonitorStatus;
  priority: boolean;
  last_checked: number | null;
  http_status: number | null;
  elapsed_ms: number | null;
  failure: string | null;
}

export interface CatalogLink extends Link, LinkOrganization {
  selection_key: string;
  recorded_redirects: number;
  last_redirect_date: string | null;
  last_redirect_at: number | null;
  /** Live redirect state from the policy, protection, and visit counter (API v1, additive). */
  state: LinkState;
  starts_at: number | null;
  expires_at: number | null;
  max_visits: number | null;
  visits_used: number;
  protection: 'password' | 'one_time' | null;
  rule_count: number;
  /** Redirects in the last 30 UTC days including today, or null when analytics is off for the link. */
  visits_30d: number | null;
  /** Redirects per UTC day for the last 14 days, oldest first. */
  trend: number[];
  monitor: LinkMonitorSummary | null;
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

export interface DestinationRule {
  host: string;
  subdomains: boolean;
  action: 'allow' | 'block';
}

export interface DestinationPolicy {
  enabled: boolean;
  rules: DestinationRule[];
}

export interface DestinationPolicyReport extends DestinationPolicy {
  impact: {
    links: number;
    rule_links: number;
    fallbacks: number;
    examples: { id: number; host: string; slug: string }[];
  };
}

export interface TrafficReport {
  day: number;
  baseline_days: number;
  minimum_redirects: number;
  multiplier: number;
  host: string | null;
  analytics_enabled: boolean;
  items: { id: number; host: string; slug: string; title: string; today: number; baseline_average: number }[];
  has_more: boolean;
}

export const scopes = ['read', 'write', 'delete', 'analytics'] as const;
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
  // `row` and `field` locate the first failing item of a multi-row request such as an import.
  constructor(readonly status: number, readonly code: string, readonly row: number | null = null, readonly field: string | null = null) {
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
      const row = Number.isSafeInteger(problem?.row) && problem.row > 0 ? problem.row as number : null;
      const field = typeof problem?.field === 'string' && /^[a-z_]{1,32}$/.test(problem.field) ? problem.field : null;
      throw new ApiError(response.status, code, row, row === null ? null : field);
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
