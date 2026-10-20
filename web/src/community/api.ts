import { ApiError, apiRequest as send } from '../api';
import type { AvatarUrl } from '../generated/community/AvatarUrl';
import type { BlockList } from '../generated/community/BlockList';
import type { Me } from '../generated/community/Me';
import type { MePatch } from '../generated/community/MePatch';
import type { ModerateProfile } from '../generated/community/ModerateProfile';
import type { PeoplePage } from '../generated/community/PeoplePage';
import type { Publication } from '../generated/community/Publication';
import type { ReportList } from '../generated/community/ReportList';
import type { UserPage } from '../generated/community/UserPage';
import type { GroupBlock } from '../generated/GroupBlock';
import { linkKeyRefused } from './link';

const c = (path: string) => `/api/community/${path}`;

/** Every Community call: a key the server refused (`device_unknown`) signs this computer out of Community. */
const apiRequest = <T,>(...args: Parameters<typeof send>): Promise<T> =>
  send<T>(...args).catch((e: unknown) => {
    if (e instanceof ApiError && e.code === 'device_unknown') linkKeyRefused();
    throw e;
  });
const id = encodeURIComponent;

export type PublishRequest = { id?: string; expected_version?: number; title: string; description: string; license: string; tags: string[]; content: GroupBlock; reviewed: true };

/** Community's API, at the same relative address in every build. */
export const communityApi = {
  me: () => apiRequest<Me>('GET', c('me')),
  updateMe: (p: MePatch) => apiRequest<Me>('PATCH', c('me'), p),
  putAvatar: (image: Blob) => apiRequest<AvatarUrl>('PUT', c('me/avatar'), undefined, { bytes: { body: image, type: image.type } }),
  deleteAvatar: () => apiRequest<AvatarUrl>('DELETE', c('me/avatar')),
  people: (q: string, offset: number) => apiRequest<PeoplePage>('GET', c(`people?q=${id(q)}&offset=${offset}`)),
  profile: (handle: string) => apiRequest<UserPage>('GET', c(`users/${id(handle)}`)),
  blocks: (q: string, offset: number) => apiRequest<BlockList>('GET', c(`blocks?q=${id(q)}&offset=${offset}`)),
  block: (block: string) => apiRequest<Publication>('GET', c(`blocks/${id(block)}`)),
  publish: (p: PublishRequest) => apiRequest<{ id: string; version: number }>('POST', c('blocks'), p),
  unpublish: (block: string) => apiRequest<unknown>('DELETE', c(`blocks/${id(block)}`)),
  save: (block: string) => apiRequest<unknown>('POST', c(`blocks/${id(block)}/save`)),
  unsave: (block: string) => apiRequest<unknown>('DELETE', c(`blocks/${id(block)}/save`)),
  saved: () => apiRequest<BlockList>('GET', c('saved')),
  mine: () => apiRequest<BlockList>('GET', c('mine')),
  reportBlock: (block: string, reason: string, target_kind: 'block' | 'profile' = 'block') => apiRequest<unknown>('POST', c(`blocks/${id(block)}/report`), { reason, target_kind }),
  reportProfile: (handle: string, reason: string) => apiRequest<unknown>('POST', c(`users/${id(handle)}/report`), { reason }),
  reports: () => apiRequest<ReportList>('GET', c('reports')),
  moderateBlock: (block: string, hidden: boolean) => apiRequest<unknown>('POST', c(`blocks/${id(block)}/moderate`), { hidden }),
  moderateProfile: (handle: string, m: ModerateProfile) => apiRequest<unknown>('POST', c(`users/${id(handle)}/moderate`), m),
};

/** A member's picture address, only when it is one of Community's own or an inline image; anything
 * else (another site, a script, an HTML data URL) is dropped and the initials show instead. */
export function safeAvatarUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  if (/^\/api\/community\/users\/[a-z0-9-]+\/avatar(\?v=[0-9a-f]+)?$/.test(url)) return url;
  if (/^data:image\/svg\+xml(;base64)?,/.test(url)) return url;
  return null;
}
