import { useState } from 'react';
import type { MeUser } from '../generated/community/MeUser';
import { useCommunity } from './context';
import { HandleStep } from './HandleStep';
import { MeGate } from './MeGate';
import { PublishForm } from './PublishForm';
import './community.css';

/** `/community/publish`: a handle first if you have none, then a group from your Library. */
export function PublishPage() {
  return (
    <section className="cm-page" aria-label="Publish">
      <h1>Publish a group</h1>
      <MeGate why="Sign in to publish.">{(me) => <Steps me={me} />}</MeGate>
    </section>
  );
}

function Steps({ me }: { me: MeUser }) {
  const { reloadMe } = useCommunity();
  const [chosen, setChosen] = useState(false);
  const [refused, setRefused] = useState(false);
  // Asked only when you have no handle, or when publishing said one is needed.
  if (refused || (!me.handle && !chosen)) return <HandleStep name={me.name} onDone={() => { setChosen(true); setRefused(false); void reloadMe(); }} />;
  return <PublishForm onHandleRequired={() => setRefused(true)} />;
}
