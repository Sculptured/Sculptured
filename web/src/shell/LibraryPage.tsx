import { GroupLibrary } from '../groups/GroupLibrary';
import { Link, useParams } from 'react-router-dom';
import { BrandMark } from './BrandMark';

export const LIBRARY_SECTIONS = {
  groups: {name:'Groups',title:'Reusable groups',description:'Create groups independently and insert them into projects.',action:'Create group',icon:'group'},
  systems: { name: 'Systems', title: 'A home for reusable systems.', description: 'Team designs, agent roles, jobs and connections, collected in one place.', action: 'Publish a system', icon: 'graph' },
  runs: { name: 'Full runs', title: 'The complete story of a run.', description: 'Inputs, handoffs, transcripts, outputs and verification evidence, kept together.', action: 'Publish a run', icon: 'activity' },
  publishing: { name: 'Publishing', title: 'Share work with its context.', description: 'A place to review what a system or run includes before making it available to others.', action: 'Create a publication', icon: 'upload' },
} as const;
export type LibrarySection = keyof typeof LIBRARY_SECTIONS;
export function librarySection(value?: string): LibrarySection {
  return value && Object.hasOwn(LIBRARY_SECTIONS, value) ? value as LibrarySection : 'systems';
}

/** Private local groups are intentionally separate from public community records. */
export function LibraryPage({section:_section}:{section:LibrarySection}) {
 return <section className="panel library-page" aria-label="Library"><header className="library-header"><div className="coastal-library-title"><BrandMark /><div><h1>Private Library</h1><p className="muted">Reusable agent groups, stored on this machine.</p></div></div><Link className="btn" to="/community">Community account</Link></header><GroupLibrary/></section>;
}

/** `/library/:collection`, as a page of its own. */
export function LibraryRoute() {
  const { collection } = useParams();
  return <main className="page page-wide library-host"><LibraryPage section={librarySection(collection)} /></main>;
}
