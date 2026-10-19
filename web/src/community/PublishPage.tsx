import { useState } from 'react';
import { useCommunity } from './context';
import { HandleStep } from './HandleStep';
import { PublishForm } from './PublishForm';
import { SignInPrompt } from './SignInPrompt';
import './community.css';

/** `/community/publish`: a handle first if you have none, then a group from your Library. */
export function PublishPage() {
  const { me, reloadMe } = useCommunity();
  const [needHandle, setNeedHandle] = useState(!me?.handle);
  if (!me) return <SignInPrompt why="Sign in to publish." />;
  return (
    <section className="cm-page" aria-label="Publish">
      <h1>Publish a group</h1>
      {needHandle
        ? <HandleStep name={me.name} onDone={() => { setNeedHandle(false); void reloadMe(); }} />
        : <PublishForm onHandleRequired={() => setNeedHandle(true)} />}
    </section>
  );
}
