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
  local_port: number;
}

export interface Link {
  id: number;
  target: string;
  slug: string;
  host: string;
  title: string;
  enabled: boolean;
  tags: string[];
  created_at: string;
}

export interface LinkPage {
  items: Link[];
  next_after_id: number | null;
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

export interface Api {
  <T>(path: string, method?: 'GET' | 'POST' | 'PATCH' | 'DELETE', data?: unknown): Promise<T>;
}

export function createApi(csrf: () => string | undefined, request = fetch): Api {
  return async function<T>(path: string, method = 'GET', data?: unknown): Promise<T> {
    const headers = new Headers({ Accept: 'application/json' });
    const token = csrf();
    if (method !== 'GET' && token) headers.set('X-CSRF-Token', token);
    if (data !== undefined) headers.set('Content-Type', 'application/json');
    const response = await request(`/api/v1${path}`, {
      method,
      credentials: 'same-origin',
      headers,
      body: data === undefined ? undefined : JSON.stringify(data),
    });
    if (!response.ok) {
      const problem = await response.json().catch(() => null);
      const code = typeof problem?.code === 'string' ? problem.code : '';
      throw new ApiError(response.status, code);
    }
    if (response.status === 204) return undefined as T;
    return response.json();
  };
}

export function shortUrl(link: Pick<Link, 'host' | 'slug'>, port: number): string {
  const local = link.host === 'localhost' || link.host === '[::1]' || /^127\.(?:\d{1,3}\.){2}\d{1,3}$/.test(link.host);
  return `${local ? `http://${link.host}:${port}` : `https://${link.host}`}/${link.slug}`;
}

export function linkName(link: Link): string {
  return link.title || `/${link.slug}`;
}
