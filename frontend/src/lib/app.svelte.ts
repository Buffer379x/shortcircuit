// Session-wide state shared by the shell and the pages: the API client, the signed-in user,
// the public domains, the tag catalog (for colors and suggestions), and navigation counts.
import type { Api, Domains, Tag, User } from '../api';
import { shortUrl as buildShortUrl } from '../api';

export const app = $state({
  api: null as unknown as Api,
  user: null as User | null,
  domains: null as Domains | null,
  tags: [] as Tag[],
  /** Name of the item shown on a detail page, for the breadcrumb. */
  crumb: '',
  counts: { links: null as number | null, unreachable: 0, domains: null as number | null, keys: null as number | null },
});

export function tagColor(name: string) {
  return app.tags.find((t) => t.name === name)?.color ?? 'var(--text-muted)';
}

export function shortUrl(link: { host: string; slug: string }) {
  return app.domains ? buildShortUrl(link, app.domains) : `https://${link.host}/${link.slug}`;
}

export async function loadTags() {
  try {
    const result = await app.api<{ items: Tag[] }>('/catalog/tags');
    app.tags = result.items;
  } catch {
    /* Tags only add colors and suggestions; pages show their own errors. */
  }
}
