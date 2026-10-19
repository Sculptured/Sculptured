import { NavLink } from 'react-router-dom';
import { COMMUNITY_WORDS } from '../vocab';
import { useCommunity } from './context';

const TABS: [string, string][] = [['/community', COMMUNITY_WORDS.tabs.explore], ['/community/people', COMMUNITY_WORDS.tabs.people], ['/community/saved', COMMUNITY_WORDS.tabs.saved], ['/community/mine', COMMUNITY_WORDS.tabs.yours]];

/** Explore · People · Saved · Yours, and Moderation for moderators. */
export function CommunityTabs() {
  const { me } = useCommunity();
  const tabs = me?.admin ? [...TABS, ['/community/moderation', COMMUNITY_WORDS.tabs.moderation] as [string, string]] : TABS;
  return (
    <nav className="cm-tabs" aria-label="Community">
      {tabs.map(([to, label]) => <NavLink key={to} to={to} end={to === '/community'} className={({ isActive }) => (isActive ? 'on' : '')}>{label}</NavLink>)}
    </nav>
  );
}
