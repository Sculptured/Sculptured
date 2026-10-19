import { errorText } from '../errors';
import { Btn } from '../shell/ui';

/** Why a Community page could not load, in plain words, with Retry. */
export function LoadProblem({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div className="cm-problem" role="alert">
      <p>{errorText(error)}</p>
      <Btn size="sm" onClick={onRetry}>Retry</Btn>
    </div>
  );
}
