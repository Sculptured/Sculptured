import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, ApiError } from '../api';
import { errorText } from '../errors';
import type { GroupBlock } from '../generated/GroupBlock';
import { sanitizeExport } from '../release/sharing';
import { Btn } from '../shell/ui';
import { TemplatePreview } from '../structure/TemplateGallery';
import { communityApi } from './api';

type Fields = { title: string; description: string; tags: string; license: string; reviewed: boolean };

/** Publishing one of your Library's groups: pick it, see exactly what goes out, describe it, confirm the review. */
export function PublishForm({ onHandleRequired }: { onHandleRequired: () => void }) {
  const [library, setLibrary] = useState<GroupBlock[] | null>(null);
  const [chosen, setChosen] = useState<GroupBlock | null>(null);
  const [f, setF] = useState<Fields>({ title: '', description: '', tags: '', license: 'MIT', reviewed: false });
  const { state, setState, publish } = usePublish(onHandleRequired);
  useEffect(() => { api.groupBlocks().then(setLibrary, (e) => setState({ busy: false, error: errorText(e) })); }, [setState]);
  const out = chosen ? sanitizeExport(chosen) : null;
  const ready = !!out && f.reviewed && !!f.title.trim() && !!f.description.trim();
  return (
    <form className="cm-page cm-publish" onSubmit={(e) => { e.preventDefault(); if (out) void publish(out, f); }}>
      <LibraryChoice library={library} chosen={chosen} onChoose={setChosen} />
      {out && <Preview block={out} />}
      <Describe f={f} onChange={setF} />
      {!ready && <p className="muted">Choose a group and confirm the review to publish.</p>}
      <Btn type="submit" variant="primary" disabled={!ready || state.busy}>Publish publicly</Btn>
      {state.error && <p className="tone-red" role="alert">{state.error}</p>}
    </form>
  );
}

/** Sends the publication and opens its page; a missing handle goes back to the handle step. */
function usePublish(onHandleRequired: () => void) {
  const navigate = useNavigate();
  const [state, setState] = useState({ busy: false, error: '' });
  const publish = async (out: GroupBlock, f: Fields) => {
    setState({ busy: true, error: '' });
    try {
      const r = await communityApi.publish({ title: f.title.trim(), description: f.description.trim(), license: f.license, tags: f.tags.split(',').map((t) => t.trim()).filter(Boolean), content: out, reviewed: true });
      navigate(`/community/blocks/${encodeURIComponent(r.id)}`);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'handle_required') return onHandleRequired();
      setState({ busy: false, error: errorText(e) });
    }
  };
  return { state, setState, publish };
}

function LibraryChoice({ library, chosen, onChoose }: { library: GroupBlock[] | null; chosen: GroupBlock | null; onChoose: (b: GroupBlock) => void }) {
  if (!library) return <p role="status" className="muted">Loading your Library…</p>;
  if (library.length === 0) return <p>Your Library has no groups yet. <a href="/library/groups">Save a group to your Library</a> first.</p>;
  return (
    <fieldset className="cm-field">
      <legend>Group to publish</legend>
      {library.map((b) => (
        <label key={b.id} className="cm-check"><input type="radio" name="block" checked={chosen?.id === b.id} onChange={() => onChoose(b)} /> {b.name} <span className="muted">({b.draft.agents.length} {b.draft.agents.length === 1 ? 'agent' : 'agents'})</span></label>
      ))}
    </fieldset>
  );
}

function Preview({ block }: { block: GroupBlock }) {
  return (
    <>
      <div className="cm-preview cm-preview-large"><TemplatePreview draft={block.draft} practice={false} /></div>
      <details open><summary>Review exactly what will be published</summary><pre className="cm-review" data-raw-evidence>{JSON.stringify(block, null, 2)}</pre></details>
    </>
  );
}

function Describe({ f, onChange }: { f: Fields; onChange: (f: Fields) => void }) {
  return (
    <>
      <label>Title<input required maxLength={150} value={f.title} onChange={(e) => onChange({ ...f, title: e.target.value })} /></label>
      <label>Description<textarea required maxLength={3000} value={f.description} onChange={(e) => onChange({ ...f, description: e.target.value })} /></label>
      <label>Tags (separated by commas)<input maxLength={240} value={f.tags} onChange={(e) => onChange({ ...f, tags: e.target.value })} /></label>
      <label>License<select value={f.license} onChange={(e) => onChange({ ...f, license: e.target.value })}><option>MIT</option><option>Apache-2.0</option><option>CC0-1.0</option></select></label>
      <label className="cm-check"><input type="checkbox" checked={f.reviewed} onChange={(e) => onChange({ ...f, reviewed: e.target.checked })} /> I reviewed every prompt for private information, own the rights to share this group, and choose to publish it under this license.</label>
      <p className="muted">Local paths, model overrides, run inputs, and external connection notes are removed. Prompt text can still contain secrets—review it.</p>
    </>
  );
}
