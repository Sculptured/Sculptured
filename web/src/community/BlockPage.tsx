import { useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { errorText } from '../errors';
import type { Publication } from '../generated/community/Publication';
import { BlockDescription, BlockDetails, BlockFeatures } from '../groups/BlockDetails';
import { downloadBlock } from '../release/sharing';
import { Btn } from '../shell/ui';
import { TemplatePreview } from '../structure/TemplateGallery';
import { COMMUNITY_WORDS } from '../vocab';
import { communityApi } from './api';
import { AuthorChip } from './AuthorChip';
import { BlockActions, projectCopy, useLibrary } from './BlockActions';
import { useCommunity } from './context';
import { opensHere } from './kinds';
import { LoadProblem } from './LoadProblem';
import './community.css';

/** `/community/blocks/:id`: one block, large, with what it does, who published it and what you can do with it. */
export function BlockPage() {
  const { id = '' } = useParams();
  const [pub, setPub] = useState<Publication | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const library = useLibrary();
  useEffect(() => { setError(null); communityApi.block(id).then(setPub, setError); }, [id, attempt]);
  if (error) return <LoadProblem error={error} onRetry={() => setAttempt((n) => n + 1)} />;
  if (!pub) return <p role="status" className="muted">Loading block…</p>;
  const copy = projectCopy(pub);
  return (
    <article className="cm-page cm-block">
      {opensHere(pub) && <div className="cm-preview cm-preview-large"><TemplatePreview draft={pub.content.draft} practice={false} /></div>}
      <h1>{pub.title}</h1>
      <p className="cm-meta"><AuthorChip author={pub.author} /> <span className="muted">v{pub.version} · {pub.license}</span></p>
      <BlockDescription text={pub.description} />
      {copy && <BlockFeatures block={copy} />}
      <BlockActions pub={pub} library={library} />
      {copy && <p><Btn size="sm" onClick={() => downloadBlock(copy)}>Download group</Btn></p>}
      {copy && <details><summary>Inspect this block</summary><BlockDetails block={copy} /></details>}
      <Report id={pub.id} />
      <Unpublish pub={pub} />
      <p className="muted cm-note">{COMMUNITY_WORDS.supplied}</p>
    </article>
  );
}

function Report({ id }: { id: string }) {
  const { signedIn } = useCommunity();
  const [state, setState] = useState({ sent: false, error: '' });
  const send = async (reason: string) => {
    try { await communityApi.reportBlock(id, reason); setState({ sent: true, error: '' }); } catch (e) { setState({ sent: false, error: errorText(e) }); }
  };
  if (!signedIn) return null;
  return (
    <details className="cm-report">
      <summary>Report this block</summary>
      {state.sent ? <p>Thanks. A moderator will look at it.</p> : (
        <form onSubmit={(e) => { e.preventDefault(); void send(new FormData(e.currentTarget).get('reason')?.toString() ?? ''); }}>
          <label>What is wrong with it?<textarea name="reason" required minLength={5} maxLength={1000} /></label>
          <Btn type="submit">Send report</Btn>
          {state.error && <p className="tone-red" role="alert">{state.error}</p>}
        </form>
      )}
    </details>
  );
}

function Unpublish({ pub }: { pub: Publication }) {
  const { me } = useCommunity();
  const [state, setState] = useState({ done: false, error: '' });
  if (!me?.handle || me.handle !== pub.author.handle) return null;
  if (state.done) return <p>Unpublished. It no longer shows in Explore.</p>;
  const go = async () => {
    try { await communityApi.unpublish(pub.id); setState({ done: true, error: '' }); } catch (e) { setState({ done: false, error: errorText(e) }); }
  };
  return <p><Btn size="sm" onClick={() => void go()}>Unpublish</Btn>{state.error && <span className="tone-red" role="alert"> {state.error}</span>}</p>;
}
