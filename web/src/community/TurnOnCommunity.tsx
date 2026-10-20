import { useState } from 'react';
import { ApiError } from '../api';
import { errorText } from '../errors';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { linkApi } from './link';
import './community.css';

const W = COMMUNITY_WORDS.optIn;

/** Community is off on this computer: what turning it on sends and what never leaves, then the choice. */
export function TurnOnCommunity({ onDone }: { onDone: (signIn: boolean) => void }) {
  const [origin, setOrigin] = useState('');
  const [state, setState] = useState<{ busy: boolean; error: unknown }>({ busy: false, error: null });
  const turnOn = async (signIn: boolean) => {
    setState({ busy: true, error: null });
    try {
      await linkApi.enable(origin.trim() || undefined);
      setState({ busy: false, error: null });
      onDone(signIn);
    } catch (e) {
      setState({ busy: false, error: e });
    }
  };
  const badServer = state.error instanceof ApiError && state.error.code === 'community_origin_invalid';
  return (
    <section className="cm-optin" aria-labelledby="cm-optin-title">
      <OptInText />
      <ServerField value={origin} onChange={setOrigin} problem={badServer ? errorText(state.error) : ''} />
      {state.error != null && !badServer && <p className="tone-red" role="alert">{errorText(state.error)}</p>}
      <div className="cm-actions">
        <Btn variant="primary" disabled={state.busy} onClick={() => void turnOn(false)}>{W.browse}</Btn>
        <Btn disabled={state.busy} onClick={() => void turnOn(true)}>{W.signIn}</Btn>
      </div>
    </section>
  );
}

function OptInText() {
  return (
    <>
      <h1 id="cm-optin-title">{W.title}</h1>
      <p>{W.intro}</p>
      <h2>{W.sendsTitle}</h2>
      <ul>{W.sends.map((t) => <li key={t}>{t}</li>)}</ul>
      <h2>{W.neverTitle}</h2>
      <ul><li>{W.never}</li></ul>
    </>
  );
}

/** Another Sculptured server (self-hosted); left empty, the usual one. */
function ServerField({ value, onChange, problem }: { value: string; onChange: (v: string) => void; problem: string }) {
  return (
    <details className="cm-server" open={problem !== '' || undefined}>
      <summary>{W.otherServer}</summary>
      <label>
        Server address
        <input value={value} onChange={(e) => onChange(e.target.value)} placeholder="https://app.usesculptured.com" inputMode="url" autoComplete="off" aria-invalid={problem !== '' || undefined} aria-describedby={problem ? 'cm-server-problem' : undefined} />
      </label>
      {problem && <p id="cm-server-problem" className="tone-red" role="alert">{problem}</p>}
    </details>
  );
}
