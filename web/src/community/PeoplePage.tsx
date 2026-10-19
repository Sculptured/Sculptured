import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import type { Profile } from '../generated/community/Profile';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { Avatar } from './Avatar';
import { LoadProblem } from './LoadProblem';
import './community.css';

/** `/community/people`: members with a handle, most published first, 24 at a time. */
export function PeoplePage() {
  const [q, setQ] = useState('');
  const { people, next, error, more, retry } = usePeople(q);
  return (
    <section className="cm-page" aria-label="People">
      <form role="search" className="cm-search" onSubmit={(e) => { e.preventDefault(); setQ(new FormData(e.currentTarget).get('q')?.toString().trim() ?? ''); }}>
        <input type="search" name="q" aria-label="Search people" placeholder="Search by name or handle" />
        <Btn type="submit">Search</Btn>
      </form>
      {error ? <LoadProblem error={error} onRetry={retry} /> : !people ? <p role="status" className="muted">Loading people…</p> : people.length === 0 ? <p className="muted">{q ? 'Nobody matches that search.' : 'Nobody has a profile yet.'}</p> : (
        <ul className="cm-people">{people.map((p) => <PersonCard key={p.handle} p={p} />)}</ul>
      )}
      {next !== null && !error && <Btn onClick={more}>More</Btn>}
    </section>
  );
}

function usePeople(q: string) {
  const [people, setPeople] = useState<Profile[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const load = (offset: number) => communityApi.people(q, offset).then((r) => { setPeople((old) => (offset === 0 ? r.people : [...(old ?? []), ...r.people])); setNext(r.next); }, setError);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { setError(null); setPeople(null); void load(0); }, [q, attempt]);
  return { people, next, error, more: () => { if (next !== null) void load(next); }, retry: () => setAttempt((n) => n + 1) };
}

function PersonCard({ p }: { p: Profile }) {
  return (
    <li className="cm-person">
      <Link to={`/community/u/${encodeURIComponent(p.handle)}`}><Avatar name={p.name} src={p.avatar_url} size={40} /><span>{p.name}</span></Link>
      <span className="muted">@{p.handle}</span>
      {p.bio && <span className="cm-bio-line">{p.bio.split('\n')[0]}</span>}
    </li>
  );
}
