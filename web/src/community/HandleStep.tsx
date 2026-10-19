import { useState } from 'react';
import { errorText } from '../errors';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { handleProblem, suggestHandle } from './handles';

/** Choosing a handle before publishing: blocks and the profile appear under it. */
export function HandleStep({ name, onDone }: { name: string; onDone: () => void }) {
  const [handle, setHandle] = useState(suggestHandle(name));
  const [problem, setProblem] = useState('');
  const use = async () => {
    const local = handleProblem(handle);
    if (local) return setProblem(local);
    try { await communityApi.updateMe({ handle }); onDone(); } catch (e) { setProblem(errorText(e)); }
  };
  return (
    <form className="cm-page" onSubmit={(e) => { e.preventDefault(); void use(); }}>
      <h2>Choose your handle</h2>
      <p>Your blocks and profile appear under it.</p>
      <label>Handle<input value={handle} aria-describedby={problem ? 'cm-step-problem' : undefined} onChange={(e) => setHandle(e.target.value.trim().toLowerCase())} /></label>
      {problem && <small id="cm-step-problem" className="tone-red">{problem}</small>}
      <Btn type="submit" variant="primary">Use this handle</Btn>
    </form>
  );
}
