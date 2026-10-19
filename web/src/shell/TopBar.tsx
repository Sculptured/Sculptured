import { Link, NavLink } from 'react-router-dom';
import { useHosted } from '../hosted/HostedGate';
import { teamStatus } from '../pages/status';
import { StatusBadge } from '../pages/StatusBadge';
import { boardPath, DOCS_URL } from '../paths';
import { openAppearance } from './AppearanceControl';
import { BrandMark } from './BrandMark';
import { SignInAlert } from './CliAccounts';
import { CommandTrigger } from './CommandPalette';
import { initials } from './initials';
import type { TeamRow } from './model';
import { Icon } from './ui';

type Props = {
  /** The open project, if any. */
  team?: string;
  rows: TeamRow[];
  onCommand: () => void;
  onHelp: () => void;
};

/** The one bar on every page: home, the project switcher, Library, Community, Account, Docs, Ctrl K and the app menu. */
export function TopBar({ team, rows, onCommand, onHelp }: Props) {
  return (
    <header className="topbar">
      <Link to="/" className="tb-brand" aria-label="Sculptured: all projects">
        <BrandMark />
        <span className="tb-brand-word">Sculptured</span>
      </Link>
      <ProjectSwitcher team={team} rows={rows} />
      <nav className="tb-nav" aria-label="Go to">
        <NavLink to="/library/groups" className={({ isActive }) => `tb-link${isActive ? ' on' : ''}`}>Library</NavLink>
        <NavLink to="/community" className={({ isActive }) => `tb-link${isActive ? ' on' : ''}`}>Community</NavLink>
        <AccountLink />
        <a href={DOCS_URL} className="tb-link" target="_blank" rel="noreferrer" data-tip="Guides and reference, in a new tab">Docs</a>
      </nav>
      <span className="tb-spacer" />
      <CommandTrigger onOpen={onCommand} />
      <AppMenu onHelp={onHelp} />
    </header>
  );
}

function AccountLink() {
  const s = useHosted();
  return (
    <NavLink to="/account" className={({ isActive }) => `tb-link${isActive ? ' on' : ''}`} data-tip="Sign-ins and usage">
      {s.kind === 'signed-in' && <span className="tb-avatar" aria-hidden="true">{initials(s.user.name)}</span>}
      Account
      <SignInAlert />
    </NavLink>
  );
}

/** Closes the `<details>` menu that holds the clicked item. */
const closeMenu = (e: React.MouseEvent) => {
  if ((e.target as HTMLElement).closest('a, button')) e.currentTarget.closest('details')?.removeAttribute('open');
};

/** The current project and its status; it lists every project, New project and All projects. */
export function ProjectSwitcher({ team, rows }: { team?: string; rows: TeamRow[] }) {
  const current = team ? rows.find((r) => r.rig === team) : undefined;
  return (
    <details className="top-menu tb-switcher">
      <summary aria-label={current ? `Project: ${current.title}. Switch project` : 'Switch project'}>
        {/* The open project's name and status live in its tab row below; the switcher just switches. */}
        <span className="tb-switch-name">Projects</span>
        <Icon name="chevron-down" size={14} className="tb-caret" />
      </summary>
      <div className="top-menu-body tb-projects" onClick={closeMenu}>
        <ul aria-label="Projects">
          {rows.map((r) => (
            <li key={r.rig}>
              <Link to={boardPath(r.rig)} aria-current={r.rig === team ? 'page' : undefined}>
                <span className="tb-p-name">{r.title}</span>
                <StatusBadge s={teamStatus(r)} />
              </Link>
            </li>
          ))}
          {rows.length === 0 && <li className="muted tb-p-empty">No projects yet.</li>}
        </ul>
        <div className="tb-p-foot">
          <Link to="/new" className="btn btn-sm"><Icon name="plus" size={14} />New project</Link>
          <Link to="/" className="btn btn-sm btn-ghost">All projects</Link>
        </div>
      </div>
    </details>
  );
}

/** The ⋯ menu: appearance, keys, and the pages a phone has no room for in the bar. */
function AppMenu({ onHelp }: { onHelp: () => void }) {
  const s = useHosted();
  return (
    <details className="top-menu tb-app">
      <summary aria-label="App menu" data-tip="App menu"><Icon name="more" size={16} /></summary>
      <div className="top-menu-body tb-app-items" onClick={closeMenu}>
        <Link to="/library/groups" className="tb-narrow-only">Library</Link>
        <Link to="/account" className="tb-narrow-only">Account</Link>
        <a href={DOCS_URL} className="tb-narrow-only" target="_blank" rel="noreferrer">Docs</a>
        <Link to="/community" className="tb-narrow-only">Community</Link>
        {s.kind === 'signed-in' && <Link to="/community/me">Your Community profile</Link>}
        {s.kind === 'signed-in' && <Link to="/computers">Computers</Link>}
        {s.kind === 'signed-in' && s.user.role === 'admin' && <Link to="/admin">Admin</Link>}
        <button type="button" onClick={openAppearance}>Appearance…</button>
        <button type="button" onClick={onHelp}>Keyboard shortcuts</button>
      </div>
    </details>
  );
}
