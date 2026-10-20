import { useEffect, useState } from 'react';
import type { Publication } from '../generated/community/Publication';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { BlockActions, useLibrary } from './BlockActions';
import { BlockCard } from './BlockCard';
import { useCommunity } from './context';
import { LoadProblem } from './LoadProblem';
import './community.css';

const PAGE = 24;

/** `/community`: published blocks, searchable by title, tag and author, 24 at a time. */
export function ExplorePage() {
  const [q, setQ] = useState('');
  const [offset, setOffset] = useState(0);
  const page = useBlocks(q, offset);
  return (
    <section className="cm-page" aria-label="Explore">
      <form role="search" className="cm-search" onSubmit={(e) => { e.preventDefault(); setQ(new FormData(e.currentTarget).get('q')?.toString().trim() ?? ''); setOffset(0); }}>
        <input type="search" name="q" aria-label="Search blocks" placeholder="Search by title, tag or author" />
        <Btn type="submit">Search</Btn>
      </form>
      {page.error ? <LoadProblem error={page.error} onRetry={page.retry} /> : <BlockGrid blocks={page.blocks} empty={q ? 'No blocks match that search.' : 'Nothing is published yet.'} />}
      <nav className="cm-pager" aria-label="Pages">
        <Btn disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}>Previous</Btn>
        <Btn disabled={!page.hasNext} onClick={() => setOffset(offset + PAGE)}>Next</Btn>
      </nav>
    </section>
  );
}

function useBlocks(q: string, offset: number) {
  const [blocks, setBlocks] = useState<Publication[] | null>(null);
  const [hasNext, setHasNext] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setError(null);
    communityApi.blocks(q, offset).then((r) => { if (live) { setBlocks(r.blocks); setHasNext(r.next != null); } }, (e) => { if (live) setError(e); });
    return () => { live = false; };
  }, [q, offset, attempt]);
  return { blocks, hasNext, error, retry: () => setAttempt((n) => n + 1) };
}

/** Cards in a grid (compact unless the member turned that off), each with its actions. */
export function BlockGrid({ blocks, empty }: { blocks: Publication[] | null; empty: string }) {
  const { me, signedIn } = useCommunity();
  const library = useLibrary(signedIn);
  if (!blocks) return <p role="status" className="muted">Loading blocks…</p>;
  if (blocks.length === 0) return <p className="muted">{empty}</p>;
  return (
    <div className={`cm-grid${me?.settings.compact === false ? '' : ' community-compact'}`}>
      {blocks.map((b) => <BlockCard key={b.id} pub={b} actions={<BlockActions pub={b} library={library} />} />)}
    </div>
  );
}
