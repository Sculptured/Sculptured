import { apiRequest } from '../api';
import { errorText } from '../errors';
import { useCommunityLink } from './link';

/** Who may share to Community from the board (a component's Share to Community): signed in to
 * Community on this computer, or to the hosted app. `csrf` is kept for callers' signature; requests
 * add the app's own token. */
export type ShareSession = { user: { name: string } | null; csrf: null };

export function useCommunitySession(): { session: ShareSession } {
  const { link } = useCommunityLink();
  const a = link?.signed_in ? link.account : null;
  return { session: { user: link?.signed_in ? { name: a?.name ?? '' } : null, csrf: null } };
}

/** A Community request from the board, with Community's answer in plain words when it is refused. */
export async function communityRequest<T>(path: string, _session?: ShareSession, method: 'GET' | 'POST' = 'GET', body?: unknown): Promise<T> {
  try {
    return await apiRequest<T>(method, `/api/community/${path}`, body);
  } catch (e) {
    throw new Error(errorText(e), { cause: e });
  }
}
