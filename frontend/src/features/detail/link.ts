// Loads one catalog link with GET /links/{id}; null when the id is unknown or outside the allowed hosts.
import { ApiError } from '../../api';
import type { CatalogLink } from '../../api';
import { app } from '../../lib/app.svelte';

export async function fetchLink(id: number): Promise<CatalogLink | null> {
  try {
    return await app.api<CatalogLink>(`/links/${id}`);
  } catch (failure) {
    if (failure instanceof ApiError && failure.status === 404) return null;
    throw failure;
  }
}
