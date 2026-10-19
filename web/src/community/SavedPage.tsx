import { useEffect, useState } from 'react';
import { errorText } from '../errors';
import type { Publication } from '../generated/community/Publication';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { communityApi } from './api';
import { BlockCard } from './BlockCard';
import { useCommunity } from './context';
import { LoadProblem } from './LoadProblem';
import { SignInPrompt } from './SignInPrompt';
import './community.css';

/** A signed-in list of blocks from `load`, or why it cannot show. */
export function useMyBlocks(load: () => Promise<{ blocks: Publication[] }>) {
  const { signedIn } = useCommunity();
  const [blocks, setBlocks] = useState<Publication[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { if (signedIn) { setError(null); load().then((r) => setBlocks(r.blocks), setError); } }, [signedIn, attempt]);
  return { signedIn, blocks, setBlocks, error, retry: () => setAttempt((n) => n + 1) };
}

/** `/community/saved`: what you saved, each with Remove save. */
export function SavedPage() {
  const { signedIn, blocks, setBlocks, error, retry } = useMyBlocks(communityApi.saved);
  const [problem, setProblem] = useState('');
  if (!signedIn) return <SignInPrompt why="Sign in to see what you saved." />;
  if (error) return <LoadProblem error={error} onRetry={retry} />;
  if (!blocks) return <p role="status" className="muted">Loading saved blocks…</p>;
  const remove = async (id: string) => {
    try { await communityApi.unsave(id); setBlocks(blocks.filter((b) => b.id !== id)); } catch (e) { setProblem(errorText(e)); }
  };
  return (
    <section className="cm-page" aria-label="Saved">
      {problem && <p className="tone-red" role="alert">{problem}</p>}
      {blocks.length === 0 ? <p className="muted">Nothing saved yet.</p> : (
        <div className="cm-grid">{blocks.map((b) => <BlockCard key={b.id} pub={b} actions={<Btn size="sm" onClick={() => void remove(b.id)}>{COMMUNITY_WORDS.unsave}</Btn>} />)}</div>
      )}
    </section>
  );
}
