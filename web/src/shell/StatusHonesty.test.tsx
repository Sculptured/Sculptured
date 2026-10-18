import { readFileSync } from 'node:fs';
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { agentState, STATES } from '../herdr';
import { seatView } from '../test-fixtures';
import { CheckMark } from './CheckMark';
import { StateMark } from './ui';

const css = (name: string) => readFileSync(`${process.cwd()}/src/${name}`, 'utf8');

// An agent saying "done" is reported complete, not verified (AGENTS.md). Green is reserved for a
// check Sculptured ran that passed; reported done is grey, in every look.
describe('reported done is never drawn as verified', () => {
  it('says "done (reported)" in the reported (grey) style, not the verified one', () => {
    const { container } = render(<StateMark state={agentState(seatView('a'), true)} pill />);
    const mark = container.querySelector('.state')!;
    expect(mark).toHaveTextContent('done (reported)');
    expect(mark).toHaveClass('tone-reported', 'st-done');
    expect(mark.className).not.toMatch(/tone-(green|verified)/);
    // No agent state at all uses the verified colour.
    for (const s of Object.values(STATES)) expect(s.tone).not.toMatch(/green|verified/);
  });

  it('keeps the verified style for a check that passed', () => {
    const check = (passed: boolean | null) => ({ command: 'npm test', passed, exit_code: passed ? 0 : 1, timed_out: false, output: '', at_ms: 1, duration_ms: 10 });
    const passed = render(<CheckMark check={check(true)} />).container.querySelector('.check-mark')!;
    expect(passed).toHaveClass('tone-verified');
    const failed = render(<CheckMark check={check(false)} />).container.querySelector('.check-mark')!;
    expect(failed).not.toHaveClass('tone-verified');
  });

  it('colours reported done with --status-done, which no look sets to green', () => {
    const styles = css('styles.css');
    // The status rules read the status tokens, and reported done reads --status-done.
    expect(styles).toMatch(/\.tone-reported \{ color: var\(--status-done\); \}/);
    expect(styles).toMatch(/\.tone-verified \{ color: var\(--status-verified\); \}/);
    expect(styles).not.toMatch(/\.st-done[^{]*\{[^}]*var\(--(green|status-verified)\)/);
    const tokens = css('tokens.css');
    const done = [...tokens.matchAll(/--status-done:\s*([^;]+);/g)].map((m) => m[1].trim());
    expect(done.length).toBeGreaterThan(0);
    for (const v of done) expect(v).not.toMatch(/green|verified/);
  });
});
