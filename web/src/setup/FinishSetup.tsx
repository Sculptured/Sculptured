import { Link } from 'react-router-dom';
import { STEP_NAME } from './steps';
import { useSetup } from './SetupGate';
import './setup.css';

/** On Projects: a small note while a setup step is neither done nor skipped, with Finish setup and Not now. */
export function FinishSetup() {
  const setup = useSetup();
  if (!setup?.ready || !setup.requiredDone) return null;
  const left = setup.steps.filter((s) => s !== 'welcome' && setup.stateOf(s) === 'todo');
  if (left.length === 0) return null;
  const notNow = async () => { for (const s of left) await setup.mark(s, 'skipped'); };
  return (
    <aside className="setup-note" aria-label="Setup">
      <span>Setup has steps left: {left.map((s) => STEP_NAME[s]).join(', ')}.</span>
      <span className="tb-fill" />
      <Link to="/setup" className="btn btn-sm btn-primary">Finish setup</Link>
      <button type="button" className="btn btn-sm btn-ghost" onClick={() => void notNow()}>Not now</button>
    </aside>
  );
}
