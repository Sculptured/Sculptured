import { useRef, useState } from 'react';
import { ApiError } from '../api';
import { errorText } from '../errors';
import type { MeUser } from '../generated/community/MeUser';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { handleProblem } from './handles';

type Problems = { handle?: string; bio?: string; links?: string; other?: string };

/** Where a refused save's words go: under the field it is about, else in a banner. */
function placeProblem(e: unknown): Problems {
  const code = e instanceof ApiError ? e.code : '';
  if (code.startsWith('handle_')) return { handle: errorText(e) };
  if (code === 'bio_too_long') return { bio: errorText(e) };
  if (code === 'link_invalid') return { links: errorText(e) };
  return { other: errorText(e) };
}

/** Name, handle, bio, links and the compact-cards setting; typed values are kept whatever happens. */
export function ProfileForm({ me, onSaved }: { me: MeUser; onSaved: () => void }) {
  const [v, setV] = useState({ name: me.name, handle: me.handle ?? '', bio: me.bio, links: [0, 1, 2].map((i) => me.links[i] ?? ''), compact: me.settings.compact });
  const [problems, setProblems] = useState<Problems>({});
  const [saved, setSaved] = useState(false);
  const handleInput = useRef<HTMLInputElement>(null);
  const save = async () => {
    setSaved(false);
    const local = v.handle !== (me.handle ?? '') ? handleProblem(v.handle) : null;
    const found: Problems = local ? { handle: local } : await send(me, v).then(() => ({}), placeProblem);
    setProblems(found);
    if (found.handle) handleInput.current?.focus();
    else if (!Object.keys(found).length) { setSaved(true); onSaved(); }
  };
  return (
    <form className="cm-profile-form" onSubmit={(e) => { e.preventDefault(); void save(); }}>
      <label>Display name<input required maxLength={80} value={v.name} onChange={(e) => setV({ ...v, name: e.target.value })} /></label>
      <Field id="cm-handle" label="Handle" hint="3–30 lowercase letters, digits or dashes. You can change it once every 30 days." problem={problems.handle}>
        {(describedBy) => <input id="cm-handle" ref={handleInput} value={v.handle} aria-describedby={describedBy} onChange={(e) => setV({ ...v, handle: e.target.value.trim().toLowerCase() })} />}
      </Field>
      <Field id="cm-bio" label="Bio" hint={`${[...v.bio].length}/280`} problem={problems.bio}>
        {(describedBy) => <textarea id="cm-bio" maxLength={280} value={v.bio} aria-describedby={describedBy} onChange={(e) => setV({ ...v, bio: e.target.value })} />}
      </Field>
      <LinksField links={v.links} problem={problems.links} onChange={(links) => setV({ ...v, links })} />
      <label className="cm-check"><input type="checkbox" checked={v.compact} onChange={(e) => setV({ ...v, compact: e.target.checked })} /> Compact block cards</label>
      <Btn type="submit" variant="primary">Save profile</Btn>
      {saved && <p role="status">Saved.</p>}
      {problems.other && <p className="tone-red" role="alert">{problems.other}</p>}
    </form>
  );
}

function send(me: MeUser, v: { name: string; handle: string; bio: string; links: string[]; compact: boolean }) {
  return communityApi.updateMe({ name: v.name, bio: v.bio, links: v.links.map((l) => l.trim()).filter(Boolean), settings: { compact: v.compact }, ...(v.handle !== (me.handle ?? '') ? { handle: v.handle } : {}) });
}

/** A labelled field whose hint and problem describe it to assistive technology. */
function Field({ id, label, hint, problem, children }: { id: string; label: string; hint: string; problem?: string; children: (describedBy: string) => React.ReactNode }) {
  return (
    <div className="cm-field">
      <label htmlFor={id}>{label}</label>
      {children(`${id}-hint${problem ? ` ${id}-problem` : ''}`)}
      <small id={`${id}-hint`} className="muted">{hint}</small>
      {problem && <small id={`${id}-problem`} className="tone-red">{problem}</small>}
    </div>
  );
}

function LinksField({ links, problem, onChange }: { links: string[]; problem?: string; onChange: (l: string[]) => void }) {
  return (
    <fieldset className="cm-field">
      <legend>Links</legend>
      {links.map((l, i) => <input key={i} type="url" placeholder="https://" aria-label={`Link ${i + 1}`} value={l} aria-invalid={problem ? true : undefined} aria-describedby={problem ? 'cm-links-problem' : undefined}
        onChange={(e) => onChange(links.map((x, j) => (j === i ? e.target.value : x)))} />)}
      {problem && <small id="cm-links-problem" className="tone-red" role="alert">{problem}</small>}
    </fieldset>
  );
}
