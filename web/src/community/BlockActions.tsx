import { useEffect, useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { GroupBlock } from '../generated/GroupBlock';
import type { Publication } from '../generated/community/Publication';
import { AddToProject } from '../groups/AddToProject';
import { track } from '../hosted/analytics';
import { sanitizeExport } from '../release/sharing';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { communityApi } from './api';
import { useCommunity } from './context';
import { ComponentTeamActions } from '../release/ComponentBlockCard';
import { componentOf, opensHere } from './kinds';
import { libraryState, saveToLibrary, type LibraryState } from './saveToLibrary';

/** The block as a project gets it: no folders, no models, restricted permissions. Null when it is not a valid group. */
export function projectCopy(pub: Publication): GroupBlock | null {
  try {
    return sanitizeExport(pub.content);
  } catch {
    return null;
  }
}

/** Add to project (two clicks, through the review dialog) and Save. Other kinds open in a later version. */
/** `library`: the Library's blocks, loaded once by the page (null while loading). */
export function BlockActions({ pub, library }: { pub: Publication; library: GroupBlock[] | null }) {
  const { workspace } = useCommunity();
  const [adding, setAdding] = useState(false);
  const component = componentOf(pub);
  if (component) return <ComponentTeamActions component={component} className="cm-actions" />;
  if (!opensHere(pub)) return <p className="muted">{COMMUNITY_WORDS.laterVersion}</p>;
  const copy = projectCopy(pub);
  return (
    <div className="cm-actions">
      <Btn variant="primary" disabled={!copy || !workspace} onClick={() => { track('add-from-community'); setAdding(true); }}>{COMMUNITY_WORDS.addToProject}</Btn>
      {!workspace && <span className="muted">Sign in to add it to a project or your Library.</span>}
      {!copy && <span className="muted">This block is not a group Sculptured can add.</span>}
      {copy && library && <LibraryButton pub={pub} library={library} />}
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

/** Save to Library, or say it is there, or offer the newer version (as an update of the same copy). */
function LibraryButton({ pub, library }: { pub: Publication; library: GroupBlock[] }) {
  const [saved, setSaved] = useState<GroupBlock | null>(null);
  const [problem, setProblem] = useState({ busy: false, error: '' });
  const state: LibraryState = saved ? { kind: 'same', copy: saved } : libraryState(pub, library);
  if (state.kind === 'same') return <><Btn disabled>{COMMUNITY_WORDS.inLibrary}</Btn><span className="muted">This version is already saved.</span></>;
  const save = async () => {
    setProblem({ busy: true, error: '' });
    try {
      setSaved(await saveToLibrary(pub));
      setProblem({ busy: false, error: '' });
    } catch (e) {
      setProblem({ busy: false, error: errorText(e) });
    }
  };
  return (
    <>
      <Btn disabled={problem.busy} onClick={() => void save()}>{state.kind === 'older' ? COMMUNITY_WORDS.updateTo(pub.version) : COMMUNITY_WORDS.saveToLibrary}</Btn>
      {problem.error && <span className="tone-red" role="alert">{problem.error}</span>}
    </>
  );
}

/** The Library's blocks, once per page and only when there is a workspace; an unreadable Library reads as empty (Save to Library still works). */
export function useLibrary(workspace: boolean): GroupBlock[] | null {
  const [library, setLibrary] = useState<GroupBlock[] | null>(null);
  useEffect(() => { if (workspace) api.groupBlocks().then(setLibrary, () => setLibrary([])); }, [workspace]);
  return library;
}
