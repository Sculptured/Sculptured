import { useState } from 'react';
import { errorText } from '../errors';
import type { GroupBlock } from '../generated/GroupBlock';
import type { Publication } from '../generated/community/Publication';
import { AddToProject } from '../groups/AddToProject';
import { sanitizeExport } from '../release/sharing';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { communityApi } from './api';
import { useCommunity } from './context';
import { opensHere } from './kinds';

/** The block as a project gets it: no folders, no models, restricted permissions. Null when it is not a valid group. */
export function projectCopy(pub: Publication): GroupBlock | null {
  try {
    return sanitizeExport(pub.content);
  } catch {
    return null;
  }
}

/** Add to project (two clicks, through the review dialog) and Save. Other kinds open in a later version. */
export function BlockActions({ pub }: { pub: Publication }) {
  const [adding, setAdding] = useState(false);
  if (!opensHere(pub)) return <p className="muted">{COMMUNITY_WORDS.laterVersion}</p>;
  const copy = projectCopy(pub);
  return (
    <div className="cm-actions">
      <Btn variant="primary" disabled={!copy} onClick={() => setAdding(true)}>{COMMUNITY_WORDS.addToProject}</Btn>
      {!copy && <span className="muted">This block is not a group Sculptured can add.</span>}
      <SaveButton pub={pub} />
      {adding && copy && <AddToProject block={copy} onClose={() => setAdding(false)} />}
    </div>
  );
}

function SaveButton({ pub }: { pub: Publication }) {
  const { signedIn } = useCommunity();
  const [saved, setSaved] = useState(false);
  const [state, setState] = useState({ busy: false, error: '' });
  if (!signedIn) return <><Btn disabled>{COMMUNITY_WORDS.save}</Btn><span className="muted">{COMMUNITY_WORDS.signInToSave}</span></>;
  const toggle = async () => {
    setState({ busy: true, error: '' });
    try {
      await (saved ? communityApi.unsave(pub.id) : communityApi.save(pub.id));
      setSaved(!saved);
      setState({ busy: false, error: '' });
    } catch (e) {
      setState({ busy: false, error: errorText(e) });
    }
  };
  return (
    <>
      <Btn disabled={state.busy} onClick={() => void toggle()}>{saved ? COMMUNITY_WORDS.unsave : COMMUNITY_WORDS.save}</Btn>
      {state.error && <span className="tone-red" role="alert">{state.error}</span>}
    </>
  );
}
