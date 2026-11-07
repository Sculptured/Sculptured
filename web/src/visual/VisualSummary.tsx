import type { ReactNode } from 'react';
import { Icon, StateMark } from '../shell/ui';
import type { AgentState } from '../herdr';
import { plural } from '../format';
import { sinceText } from '../progress/activity';
import { KIND_ICON } from '../shell/ToolCard';
import { KIND_WORD, TOOL_KINDS } from './tools';
import type { deriveProcess } from './process';
import { PHASE_WORD, Running } from './visualParts';

/** What the agent is doing now, or did last, with a way to that turn. */
export function NowBar({ p, now, onShow }: { p: ReturnType<typeof deriveProcess>; now: number; onShow: (n: number) => void }) {
  const last = p.phases[p.phases.length - 1];
  if (!last) return null;
  const step = [...last.steps].reverse().find((s) => s.status === 'running');
  const running = last.status === 'running';
  return (
    <div className={`vp-now${running ? ' live' : ''}`}>
      <span className="vp-now-label">{running ? 'Now' : 'Last'}</span>
      <span className="vp-now-text">
        {last.turnId === null ? 'Outside a turn' : `Turn ${last.n}`} · {running && <Running />}
        {PHASE_WORD[last.status]}
        {step ? (
          <>
            {' · '}
            <Icon name={KIND_ICON[step.kind]} size={12} />{' '}
            <span data-raw-evidence>
              {step.name} {step.summary}
            </span>
          </>
        ) : running ? (
          ' · thinking'
        ) : last.endMs !== null ? (
          ` · ${sinceText(last.endMs, now)}`
        ) : null}
      </span>
      <button type="button" className="link-btn" onClick={() => onShow(last.n)}>
        Show turn
      </button>
    </div>
  );
}

function Tile({ label, wide, children }: { label: string; wide?: boolean; children: ReactNode }) {
  return (
    <div className={`vp-tile${wide ? ' wide' : ''}`}>
      <span className="vp-tile-label">{label}</span>
      {children}
    </div>
  );
}

function ToolCallsTile({ s }: { s: Summary }) {
  const total = s.toolCalls || 1;
  const kinds = TOOL_KINDS.filter((k) => s.byKind[k]);
  return (
    <Tile label="Tool calls" wide>
      <span className="vp-tile-value">{s.toolCalls}</span>
      {s.failedSteps > 0 && <span className="vp-tile-sub tone-red"> · ✗ {s.failedSteps} failed</span>}
      {s.toolCalls > 0 && (
        <>
          <span className="vp-kindbar" aria-hidden="true">
            {kinds.map((k) => (
              <span key={k} className={`k-${k}`} style={{ flexGrow: s.byKind[k] / total }} />
            ))}
          </span>
          <span className="vp-kindlegend">
            {kinds.map((k) => (
              <span key={k} className={`vp-kindkey k-${k}`}>
                <Icon name={KIND_ICON[k]} size={11} />
                {KIND_WORD[k]} {s.byKind[k]}
              </span>
            ))}
          </span>
        </>
      )}
    </Tile>
  );
}

type Summary = ReturnType<typeof deriveProcess>['summary'];

export function Summary({ p, state, now }: { p: ReturnType<typeof deriveProcess>; state: AgentState; now: number }) {
  const s = p.summary;
  return (
    <section className="vp-summary" aria-label="Summary">
      <Tile label="State"><span className="vp-tile-value"><StateMark state={state} /></span></Tile>
      <Tile label="Turns"><span className="vp-tile-value">{s.turns}</span></Tile>
      <ToolCallsTile s={s} />
      <Tile label="Files changed">
        <span className="vp-tile-value">{s.filesChanged}</span>
        <span className="vp-tile-sub"> · {s.filesRead} read</span>
      </Tile>
      <Tile label="Commands"><span className="vp-tile-value">{s.commands}</span></Tile>
      <Tile label="Checks">
        <span className="vp-tile-value">{s.checks}</span>
        {s.checksWithError > 0 && <span className="vp-tile-sub tone-red"> · {plural(s.checksWithError, 'error result')}</span>}
      </Tile>
      <Tile label="Last activity">
        <span className="vp-tile-value small" title={s.lastActivityMs === null ? undefined : new Date(s.lastActivityMs).toLocaleString()}>
          {sinceText(s.lastActivityMs, now)}
        </span>
      </Tile>
    </section>
  );
}
