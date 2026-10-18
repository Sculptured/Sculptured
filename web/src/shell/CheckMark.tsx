import { useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { SeatCheck } from '../generated/SeatCheck';
import { Btn } from './ui';

/** One word for the latest check, from the command's own exit status (Sculptured ran it). */
export function checkWords(c: SeatCheck): { word: string; tone: string; glyph: string } {
  if (c.passed === null) return { word: 'checking', tone: 'tone-amber', glyph: '◌' };
  if (c.passed) return { word: 'check passed', tone: 'tone-verified', glyph: '✓' };
  return { word: c.timed_out ? 'check timed out' : 'check failed', tone: 'tone-red', glyph: '✗' };
}

const when = (c: SeatCheck) => new Date(c.at_ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

export function CheckMark({ check }: { check: SeatCheck }) {
  const w = checkWords(check);
  const tip = `Sculptured ran “${check.command}” at ${when(check)}${check.exit_code !== null ? ` · exit ${check.exit_code}` : ''}`;
  return <span className={`check-mark ${w.tone}`} title={tip} data-check={check.passed === null ? 'running' : check.passed ? 'passed' : 'failed'}><span aria-hidden="true">{w.glyph} </span>{w.word}</span>;
}

/** The agent page's check: command, result, output, and Run check now. */
export function CheckDetails({ seat, check }: { seat: string; check?: SeatCheck }) {
  const [error, setError] = useState('');
  if (!check) return null;
  const run = async () => { setError(''); try { await api.runCheck(seat); } catch (e) { setError(errorText(e)); } };
  return (
    <section className="check-details" aria-label="Check">
      <div className="check-line">
        <strong>Check</strong> <code>{check.command}</code>
        <CheckMark check={check} />
        <Btn size="sm" onClick={() => void run()} disabled={check.passed === null}>Run check now</Btn>
      </div>
      {check.passed !== null && (
        <details>
          <summary>Output · {when(check)}{check.duration_ms !== null ? ` · ${(check.duration_ms / 1000).toFixed(1)} s` : ''}{check.exit_code !== null ? ` · exit ${check.exit_code}` : ''}</summary>
          <pre data-raw-evidence>{check.output || '(no output)'}</pre>
        </details>
      )}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </section>
  );
}
