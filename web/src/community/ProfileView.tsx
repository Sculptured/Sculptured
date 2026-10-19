import type { UserPage } from '../generated/community/UserPage';
import { Avatar } from './Avatar';
import { BlockGrid } from './ExplorePage';

/** A link's host name, for https links only (anything else is not shown as a link). */
function host(url: string): string | null {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' ? u.hostname : null;
  } catch {
    return null;
  }
}

const joined = (iso: string) => new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' }).format(new Date(iso));

/** A member's public profile: picture, name, handle, bio, links, counts and their blocks. Empty fields show nothing. */
export function ProfileView({ profile: p, blocks }: UserPage) {
  const links = p.links.map((l) => ({ l, h: host(l) })).filter((x) => x.h);
  return (
    <article className="cm-page cm-profile">
      <header className="cm-profile-head">
        <Avatar name={p.name} src={p.avatar_url} size={96} />
        <div>
          <h1>{p.name}</h1>
          <p className="muted">@{p.handle} · Joined {joined(p.joined_at)}</p>
        </div>
      </header>
      {p.bio && <p className="cm-bio">{p.bio}</p>}
      {links.length > 0 && <ul className="cm-links">{links.map(({ l, h }) => <li key={l}><a href={l} rel="nofollow noopener noreferrer ugc" target="_blank">{h}</a></li>)}</ul>}
      <p className="muted">{p.counts.published} {p.counts.published === 1 ? 'block' : 'blocks'} published · saved {p.counts.saved_by_others} {p.counts.saved_by_others === 1 ? 'time' : 'times'}</p>
      <BlockGrid blocks={blocks} empty="Nothing published yet." />
    </article>
  );
}
