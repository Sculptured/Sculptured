import { useState } from 'react';
import { Link } from 'react-router-dom';
import { errorText } from '../errors';
import type { Publication } from '../generated/community/Publication';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { BlockCard } from './BlockCard';
import { LoadProblem } from './LoadProblem';
import { useMyBlocks } from './SavedPage';
import { SignInPrompt } from './SignInPrompt';
import './community.css';

/** `/community/mine`: what you published, each with Unpublish (after you confirm). */
export function YoursPage() {
  const { signedIn, blocks, setBlocks, error, retry } = useMyBlocks(communityApi.mine);
  if (!signedIn) return <SignInPrompt why="Sign in to see what you published." />;
  if (error) return <LoadProblem error={error} onRetry={retry} />;
  if (!blocks) return <p role="status" className="muted">Loading your blocks…</p>;
  return (
    <section className="cm-page" aria-label="Yours">
      <p><Link className="btn btn-primary" to="/community/publish">Publish a group</Link></p>
      {blocks.length === 0 ? <p className="muted">You have not published anything yet.</p> : (
        <div className="cm-grid">{blocks.map((b) => <BlockCard key={b.id} pub={b} actions={<Unpublish pub={b} onDone={() => setBlocks(blocks.filter((x) => x.id !== b.id))} />} />)}</div>
      )}
    </section>
  );
}

function Unpublish({ pub, onDone }: { pub: Publication; onDone: () => void }) {
  const [asking, setAsking] = useState(false);
  const [problem, setProblem] = useState('');
  const go = async () => {
    try { await communityApi.unpublish(pub.id); onDone(); } catch (e) { setProblem(errorText(e)); }
  };
  if (!asking) return <Btn size="sm" onClick={() => setAsking(true)}>Unpublish</Btn>;
  return (
    <div className="cm-actions">
      <span>Unpublish {pub.title}? It disappears from Explore.</span>
      <Btn size="sm" variant="primary" onClick={() => void go()}>Yes, unpublish</Btn>
      <Btn size="sm" onClick={() => setAsking(false)}>Cancel</Btn>
      {problem && <span className="tone-red" role="alert">{problem}</span>}
    </div>
  );
}
