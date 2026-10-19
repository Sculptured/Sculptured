import { Link, Outlet } from 'react-router-dom';
import { BrandMark } from '../shell/BrandMark';
import './community.css';

/** Community for someone not signed in: the brand, Community, and Sign in, around the page. */
export function CommunityFrame() {
  return (
    <div className="cm-frame">
      <header className="topbar">
        <a href="/" className="tb-brand" aria-label="Sculptured"><BrandMark /><span className="tb-brand-word">Sculptured</span></a>
        <Link to="/community" className="tb-link">Community</Link>
        <span className="tb-spacer" />
        <a href="/login" className="btn btn-sm">Sign in</a>
      </header>
      <Outlet />
    </div>
  );
}
