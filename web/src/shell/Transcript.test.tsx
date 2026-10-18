import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { buildBlocks, chainSummary, Transcript } from './Transcript';
import { openRigWords } from '../test-vocab';
import { conversationItems } from '../conversation';
import { ev } from '../test-fixtures';
import { vendor } from '../herdr';
import type { EventRecord } from '../generated/EventRecord';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn() } };
});

const claude = vendor('claude-code');
const text = (seq: number, t: string) => ev(seq, { type: 'agent-text', turn_id: 't', text: t });

describe('Transcript chat', () => {
  beforeEach(() => vi.mocked(api.latestSeatEvents).mockReset());
  const at = (e: EventRecord, ms: number) => ({ ...e, at_ms: ms });
  const job = (seq: number, id: string, t: string) => ({
    ...ev(seq, { type: 'message-queued', message_id: id, intent: 'inform', text: t }),
    actor: { kind: 'system' as const, id: 'team-start' },
    provenance: 'operator' as const,
  });

  it('shows your message as a bubble, the reply as Markdown under the agent’s name, and a turn divider', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'tenure-started', tenure: 1, cause: 'fresh', driver: 'claude', native_session_id: null, caps: {} as never }),
      {...at(ev(2, { type: 'message-queued', message_id: 'm1', intent: 'inform', text: 'run the tests' }), 900), actor:{kind:'human',id:'operator'}},
      at(ev(3, { type: 'turn-started', turn_id: 't' }), 1000),
      ev(4, { type: 'message-consumed', message_id: 'm1', turn_id: 't' }),
      { ...text(5, '## Result\n\nAll **12** tests pass.'), provenance: 'agent-reported' },
      ev(6, { type: 'usage-reported', turn_id: 't', input_tokens: 1200, output_tokens: 300, cache_read_tokens: 0, cache_write_tokens: 0 }),
      at(ev(7, { type: 'turn-ended', turn_id: 't', status: 'completed' }), 4500),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} name="Builder" />);
    const convo = await screen.findByRole('list', { name: 'Conversation' });
    await screen.findByRole('heading', { name: 'Result' });
    const blocks = within(convo).getAllByRole('listitem').filter((li) => li.parentElement === convo);
    expect(blocks[0]).toHaveTextContent('Session #1 started · new conversation · Claude');
    expect(blocks[1].querySelector('.bubble')).toHaveTextContent('You: run the tests');
    expect(blocks[1]).toHaveTextContent('read');
    expect(blocks[2]).toHaveTextContent('Builder');
    expect(within(blocks[2]).getByText('12').tagName).toBe('STRONG');
    expect(blocks[2].querySelector('.reply-text')).toHaveAttribute('data-raw-evidence');
    // Agent claims are reported, never verified.
    expect(blocks[2]).toHaveTextContent('◇ reported');
    expect(blocks[2]).not.toHaveTextContent('verified');
    expect(blocks[3]).toHaveTextContent('Turn completed · 3.5s · 1.5k tokens');
    expect(openRigWords(document.body)).toEqual([]);
    // The verbatim text is one click away.
    await userEvent.setup().click(within(blocks[2]).getByRole('button', { name: 'Raw text' }));
    expect(blocks[2].querySelector('pre.reply-raw')).toHaveTextContent('## Result All **12** tests pass.');
  });

  it('says when a turn’s tokens or duration are unknown', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([ev(1, { type: 'turn-ended', turn_id: 't', status: 'failed' })]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    expect(await screen.findByText(/Turn failed/)).toHaveTextContent('✗ Turn failed · duration unknown · tokens unknown');
  });

  it('shows a tool call as a card with its kind, summary, duration and status, expanding to output', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      at(ev(1, { type: 'tool-called', turn_id: 't', call_id: 'c', name: 'Bash', input: { command: 'npm test' }, parent_call_id: null }), 1000),
      at(ev(2, { type: 'tool-resulted', turn_id: 't', call_id: 'c', output: '12 passed', is_error: false }), 1400),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const head = await screen.findByRole('button', { name: /Bash.*npm test/ });
    expect(head).toHaveTextContent('0.4s');
    expect(head).toHaveTextContent('✓ ok');
    expect(head).toHaveAttribute('aria-expanded', 'false');
    expect(within(head).getByText('npm test').closest('[data-raw-evidence]')).not.toBeNull();
    await userEvent.setup().click(head);
    expect(head).toHaveAttribute('aria-expanded', 'true');
    const out = screen.getByText('12 passed');
    expect(out.closest('[data-raw-evidence]')).not.toBeNull();
    expect(screen.getByText('Command').parentElement).toHaveTextContent('$ npm test');
  });

  it('marks helper calls, keeps long commands to one line and is a polite live region', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'tool-called', turn_id: 't', call_id: 'c', name: 'Bash', input: { command: 'echo one\necho two' }, parent_call_id: 'p' }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const head = await screen.findByRole('button', { name: /echo one/ });
    expect(head).toHaveTextContent('↳ helper');
    expect(head).toHaveTextContent('echo one …');
    expect(head).toHaveTextContent('running');
    expect(screen.getByRole('list', { name: 'Conversation' })).toHaveAttribute('aria-live', 'polite');
  });

  it('folds consecutive tool calls into "Used N tools", and shows an edit as a diff', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'tool-called', turn_id: 't', call_id: 'r', name: 'Read', input: { file_path: 'src/a.ts' }, parent_call_id: null }),
      ev(2, { type: 'tool-resulted', turn_id: 't', call_id: 'r', output: 'const a = 1;', is_error: false }),
      ev(3, { type: 'tool-called', turn_id: 't', call_id: 'e', name: 'Edit', input: { file_path: 'src/a.ts', old_string: 'const a = 1;', new_string: 'const a = 2;' }, parent_call_id: null }),
      ev(4, { type: 'tool-resulted', turn_id: 't', call_id: 'e', output: 'ok', is_error: false }),
      ev(5, { type: 'tool-called', turn_id: 't', call_id: 'b', name: 'Bash', input: { command: 'npm test' }, parent_call_id: null }),
      ev(6, { type: 'tool-resulted', turn_id: 't', call_id: 'b', output: 'Exit code 1', is_error: true }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const group = await screen.findByRole('button', { name: /Used 3 tools/ });
    expect(group).toHaveTextContent('✗ 1 failed');
    expect(group).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('button', { name: /Edit/ })).toBeNull();
    const user = userEvent.setup();
    await user.click(group);
    await user.click(screen.getByRole('button', { name: /Edit.*src\/a\.ts/ }));
    const diff = document.querySelector('pre.diff')!;
    expect(diff.querySelector('.dl-del')).toHaveTextContent('const a = 1;');
    expect(diff.querySelector('.dl-add')).toHaveTextContent('const a = 2;');
    expect(screen.getByRole('button', { name: /Bash.*npm test/ })).toHaveTextContent('✗ error');
  });

  it('cuts long output with "Show more"', async () => {
    const long = Array.from({ length: 60 }, (_, i) => `line ${i + 1}`).join('\n');
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'tool-called', turn_id: 't', call_id: 'c', name: 'Bash', input: { command: 'seq 60' }, parent_call_id: null }),
      ev(2, { type: 'tool-resulted', turn_id: 't', call_id: 'c', output: long, is_error: false }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: /seq 60/ }));
    const out = document.querySelector('pre.tool-result')!;
    expect(out).toHaveTextContent('line 24');
    expect(out).not.toHaveTextContent('line 25');
    await user.click(screen.getByRole('button', { name: 'Show more (36 more lines)' }));
    expect(document.querySelector('pre.tool-result')).toHaveTextContent('line 60');
  });

  it('shows a message the agent never read as failed, with the reason', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'message-queued', message_id: 'm1', intent: 'inform', text: 'please read me' }),
      ev(2, { type: 'tenure-ended', tenure: 1, exit_code: null, signal: 9, stderr_tail: '' }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const msg = (await screen.findByText('please read me')).closest('li')!;
    expect(msg).toHaveTextContent('✗ failed — the agent stopped before reading this message');
    expect(msg).not.toHaveTextContent('queued');
  });

  it('shows the team Job as a card sent at start, and a failed one as not sent with the reason', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      job(1, 'j1', 'Build the change'),
      ev(2, { type: 'message-consumed', message_id: 'j1', turn_id: 't' }),
      job(3, 'j2', 'Review the change'),
      ev(4, { type: 'message-failed', message_id: 'j2', reason: 'the agent never started: Codex is not available yet' }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const sent = (await screen.findByText('Build the change')).closest('li')!;
    expect(sent).toHaveTextContent('Job · sent at start');
    expect(sent).toHaveTextContent('read');
    const failed = screen.getByText('Review the change').closest('li')!;
    expect(failed).toHaveTextContent('Job · not sent');
    expect(failed).toHaveTextContent('✗ not sent — the agent never started: Codex is not available yet');
    expect(failed).not.toHaveTextContent('sent at start');
    expect(within(failed).getByText(/Codex is not available yet/)).toHaveAttribute('data-raw-evidence');
  });

  it('shows a handoff as a card from its upstream agents, each result collapsible', async () => {
    const t = "Handed over by Planner, Researcher\nBuild the login page.\n--- Planner's result ---\n1. Add a **form**\n--- Researcher's result ---\nUse OAuth.";
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      { ...ev(1, { type: 'message-queued', message_id: 'h', intent: 'inform', text: t }), actor: { kind: 'system' as const, id: 'handoff' } },
      { ...ev(2, { type: 'message-queued', message_id: 'h2', intent: 'inform', text: 'More work from Planner' }), actor: { kind: 'system' as const, id: 'handoff' } },
      ev(3, { type: 'message-failed', message_id: 'h2', reason: 'the agent is stopped' }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const card = (await screen.findByText('Handed over by Planner, Researcher')).closest('li')!;
    expect(within(card).getByText('Job')).toBeInTheDocument();
    expect(within(card).getByText('Build the login page.').closest('[data-raw-evidence]')).not.toBeNull();
    const results = card.querySelectorAll('details.msg-card-result');
    expect(results).toHaveLength(2);
    expect(results[0].querySelector('summary')).toHaveTextContent('Planner’s result');
    await userEvent.setup().click(results[0].querySelector('summary')!);
    expect(within(results[0] as HTMLElement).getByText('form').tagName).toBe('STRONG');
    const later = screen.getByText('More work from Planner').closest('li')!;
    expect(later).toHaveTextContent('✗ not sent — the agent is stopped');
  });

  it('labels heuristic provenance with its own tier, and shows alarming lifecycle changes in red', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      { ...ev(1, { type: 'seat-lifecycle-changed', lifecycle: 'attention', reason: 'no output for 10 min' }), provenance: 'heuristic' },
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const line = (await screen.findByText(/no output for 10 min/)).closest('li')!;
    expect(line).toHaveTextContent('Needs attention — no output for 10 min');
    expect(line).toHaveTextContent('≈ heuristic');
    expect(line.querySelector('.divider.alarm')).not.toBeNull();
  });

  it('shows Thinking… while a turn is open and Working… while its tool runs', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([ev(1, { type: 'turn-started', turn_id: 't' })]);
    const { rerender } = render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    expect(await screen.findByText('Thinking…')).toBeInTheDocument();
    rerender(
      <Transcript seatId="dev.a@r" feed={[ev(2, { type: 'tool-called', turn_id: 't', call_id: 'c', name: 'Bash', input: { command: 'sleep 1' }, parent_call_id: null })]} epoch={0} vendor={claude} />,
    );
    expect(await screen.findByText('Working…')).toBeInTheDocument();
    rerender(<Transcript seatId="dev.a@r" feed={[ev(3, { type: 'turn-ended', turn_id: 't', status: 'completed' })]} epoch={0} vendor={claude} />);
    await vi.waitFor(() => expect(screen.queryByText(/Thinking…|Working…/)).toBeNull());
  });

  it('searches with / and highlights every match, stepping with Enter', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      text(1, 'The login page is ready.'),
      ev(2, { type: 'message-queued', message_id: 'm', intent: 'inform', text: 'check login again' }),
      ev(3, { type: 'tool-called', turn_id: 't', call_id: 'a', name: 'Read', input: { file_path: 'a.ts' }, parent_call_id: null }),
      ev(4, { type: 'tool-called', turn_id: 't', call_id: 'b', name: 'Bash', input: { command: 'grep login src' }, parent_call_id: null }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    await screen.findByText(/The login page/);
    const user = userEvent.setup();
    document.body.focus();
    await user.keyboard('/');
    const box = screen.getByRole('searchbox', { name: 'Search transcript' });
    expect(box).toHaveFocus();
    await user.keyboard('LOGIN');
    // The group holding a matching call opens so its match can be seen.
    expect(screen.getByRole('button', { name: /Used 2 tools/ })).toHaveAttribute('aria-expanded', 'true');
    const marks = () => [...document.querySelectorAll('mark.hit')];
    expect(marks().map((m) => m.textContent)).toEqual(['login', 'login', 'login']);
    expect(screen.getByText('1 of 3')).toBeInTheDocument();
    expect(marks()[0]).toHaveClass('current');
    await user.keyboard('{Enter}');
    expect(screen.getByText('2 of 3')).toBeInTheDocument();
    expect(marks()[1]).toHaveClass('current');
    await user.keyboard('{Shift>}{Enter}{/Shift}{Shift>}{Enter}{/Shift}');
    expect(screen.getByText('3 of 3')).toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(box).toHaveValue('');
    expect(marks()).toHaveLength(0);
  });

  it('jumps to a focused item: opens its group and card, flashes it, and Esc goes back', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'tool-called', turn_id: 't', call_id: 'a', name: 'Read', input: { file_path: 'a.ts' }, parent_call_id: null }),
      ev(2, { type: 'tool-called', turn_id: 't', call_id: 'b', name: 'Bash', input: { command: 'npm test' }, parent_call_id: null }),
      ev(3, { type: 'tool-resulted', turn_id: 't', call_id: 'b', output: 'all good', is_error: false }),
    ]);
    const back = vi.fn();
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} focusKey="t-b-2" onReturn={back} returnLabel="Visual progress" />);
    const card = await vi.waitFor(() => {
      const el = document.querySelector('[data-item-key="t-b-2"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });
    expect(card).toHaveClass('flash');
    expect(within(card).getByText('all good')).toBeVisible();
    expect(card.closest('li')).toHaveFocus();
    await userEvent.setup().keyboard('{Escape}');
    expect(back).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: /Visual progress/ })).toBeInTheDocument();
  });

  it('moves focus between blocks with the arrow keys so evidence is reachable by keyboard', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      text(1, 'one'),
      ev(2, { type: 'message-queued', message_id: 'm', intent: 'inform', text: 'two' }),
    ]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    await screen.findByText('two');
    const convo = screen.getByRole('list', { name: 'Conversation' });
    convo.focus();
    const user = userEvent.setup();
    await user.keyboard('{ArrowDown}');
    expect(screen.getByText('one').closest('li.cv')).toHaveFocus();
    await user.keyboard('{ArrowDown}');
    expect(screen.getByText('two').closest('li.cv')).toHaveFocus();
  });
});

describe('buildBlocks', () => {
  it('joins quiet session events into one divider, keeps alarms apart, and folds a reply’s parts together', () => {
    const blocks = buildBlocks(
      conversationItems([
        ev(1, { type: 'seat-lifecycle-changed', lifecycle: 'launching', reason: null }),
        ev(2, { type: 'tenure-started', tenure: 1, cause: 'fresh', driver: 'claude', native_session_id: null, caps: {} as never }),
        ev(3, { type: 'seat-lifecycle-changed', lifecycle: 'running', reason: null }),
        text(4, 'hi'),
        ev(5, { type: 'tool-called', turn_id: 't', call_id: 'a', name: 'Read', input: {}, parent_call_id: null }),
        ev(6, { type: 'tool-called', turn_id: 't', call_id: 'b', name: 'Bash', input: {}, parent_call_id: null }),
        text(7, 'done'),
        ev(8, { type: 'tenure-ended', tenure: 1, exit_code: 3, signal: null, stderr_tail: '' }),
        ev(9, { type: 'seat-lifecycle-changed', lifecycle: 'attention', reason: 'exited' }),
      ]),
    );
    expect(blocks.map((b) => b.kind)).toEqual(['divider', 'agent', 'divider', 'divider']);
    expect(blocks[0].kind === 'divider' && blocks[0].items.map((n) => n.text)).toEqual(['launching', 'Session #1 started · new conversation · Claude', 'running']);
    expect(blocks[1].kind === 'agent' && blocks[1].parts.map((p) => p.kind)).toEqual(['text', 'tools', 'text']);
    expect(blocks[2].kind === 'divider' && blocks[2].items).toHaveLength(1);
    expect(blocks[3].kind === 'divider' && blocks[3].items).toHaveLength(1);
  });
});

describe('Transcript scrolling', () => {
  beforeEach(() => vi.mocked(api.latestSeatEvents).mockReset());

  /** jsdom has no layout: give the scroller a fixed geometry. */
  function geometry(el: HTMLElement, height: () => number, client = 100) {
    let top = 0;
    Object.defineProperty(el, 'scrollHeight', { configurable: true, get: height });
    Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => client });
    Object.defineProperty(el, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => (top = Math.max(0, Math.min(v, height() - client))) });
  }

  it('sticks to the bottom while you are at the bottom', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([text(1, 'first')]);
    const { rerender } = render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    await screen.findByText('first');
    const el = screen.getByTestId('transcript-scroll');
    let h = 500;
    geometry(el, () => h);
    el.scrollTop = 400;
    fireEvent.scroll(el);
    h = 600;
    rerender(<Transcript seatId="dev.a@r" feed={[text(2, 'second')]} epoch={0} vendor={claude} />);
    await screen.findByText('second');
    expect(el.scrollTop).toBe(500);
    expect(screen.queryByRole('button', { name: /new/ })).not.toBeInTheDocument();
  });

  it('stays put and counts new lines with a "▼ N new" chip when you scrolled up', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([text(1, 'first')]);
    const { rerender } = render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    await screen.findByText('first');
    const el = screen.getByTestId('transcript-scroll');
    let h = 500;
    geometry(el, () => h);
    el.scrollTop = 100;
    fireEvent.scroll(el);
    h = 700;
    rerender(<Transcript seatId="dev.a@r" feed={[text(2, 'second'), text(3, 'third')]} epoch={0} vendor={claude} />);
    await screen.findByText('third');
    expect(el.scrollTop).toBe(100);
    const chip = screen.getByRole('button', { name: '▼ 2 new' });
    await userEvent.setup().click(chip);
    expect(el.scrollTop).toBe(600);
    expect(screen.queryByRole('button', { name: /new/ })).not.toBeInTheDocument();
  });

  it('clears the chip when you scroll back to the bottom yourself', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([text(1, 'first')]);
    const { rerender } = render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    await screen.findByText('first');
    const el = screen.getByTestId('transcript-scroll');
    let h = 500;
    geometry(el, () => h);
    el.scrollTop = 0;
    fireEvent.scroll(el);
    h = 600;
    rerender(<Transcript seatId="dev.a@r" feed={[text(2, 'second')]} epoch={0} vendor={claude} />);
    expect(await screen.findByRole('button', { name: '▼ 1 new' })).toBeInTheDocument();
    act(() => {
      el.scrollTop = 500;
      fireEvent.scroll(el);
    });
    expect(screen.queryByRole('button', { name: /new/ })).not.toBeInTheDocument();
  });
});

describe('Transcript meta, folded', () => {
  beforeEach(() => vi.mocked(api.latestSeatEvents).mockReset());
  const restart = [
    ev(1, { type: 'seat-lifecycle-changed', lifecycle: 'stopping', reason: null }),
    ev(2, { type: 'tenure-ended', tenure: 1, exit_code: 0, signal: null, stderr_tail: '' }),
    ev(3, { type: 'seat-lifecycle-changed', lifecycle: 'stopped', reason: null }),
    ev(4, { type: 'seat-lifecycle-changed', lifecycle: 'launching', reason: null }),
    ev(5, { type: 'tenure-started', tenure: 2, cause: 'fresh', driver: 'stub', native_session_id: null, caps: {} as never }),
    ev(6, { type: 'seat-lifecycle-changed', lifecycle: 'running', reason: null }),
  ];

  it('folds a run of session events to one line that opens to the verbatim chain', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue(restart);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    const toggle = await screen.findByRole('button', { name: 'Restarted · new conversation' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    const chain = document.querySelector('.divider-chain')!;
    expect(chain).toHaveAttribute('data-raw-evidence');
    expect(chain).not.toBeVisible();
    // Kept in the page as evidence, and searchable.
    expect(chain).toHaveTextContent('Session #1 ended · exit 0');
    await userEvent.setup().click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(chain).toBeVisible();
    expect(chain).toHaveTextContent('Stopping › Session #1 ended · exit 0 › Stopped › Launching › Session #2 started · new conversation · Practice agent › Running');
  });

  it('says what a chain did in a few words', () => {
    const items = (es: EventRecord[]) => {
      const b = buildBlocks(conversationItems(es))[0];
      return b.kind === 'divider' ? b.items : [];
    };
    expect(chainSummary(items(restart))).toBe('Restarted · new conversation');
    expect(chainSummary(items(restart.slice(3)))).toBe('Started · new conversation');
    expect(chainSummary(items(restart.slice(0, 3)))).toBe('Session ended');
  });

  it('shows a turn end as one small line with no rules', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([ev(1, { type: 'turn-ended', turn_id: 't', status: 'completed' })]);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} />);
    expect(await screen.findByText(/Turn completed · duration unknown/)).toBeInTheDocument();
    expect(document.querySelector('.divider.turn button')).toBeNull();
  });

  it('in a header slot, search is an icon that / or a click opens, and folds back when left empty', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([text(1, 'The login page is ready.')]);
    const slot = document.createElement('div');
    document.body.appendChild(slot);
    render(<Transcript seatId="dev.a@r" feed={[]} epoch={0} vendor={claude} toolsSlot={slot} />);
    await screen.findByText(/The login page/);
    expect(document.querySelector('.cv-tools')).toBeNull();
    const icon = within(slot).getByRole('button', { name: 'Search transcript' });
    expect(icon).toHaveAttribute('aria-keyshortcuts', '/');
    expect(within(slot).queryByRole('searchbox')).toBeNull();
    const user = userEvent.setup();
    document.body.focus();
    await user.keyboard('/');
    const box = within(slot).getByRole('searchbox', { name: 'Search transcript' });
    expect(box).toHaveFocus();
    await user.keyboard('login');
    expect(screen.getByText('1 of 1')).toBeInTheDocument();
    await user.keyboard('{Escape}{Escape}');
    expect(within(slot).queryByRole('searchbox')).toBeNull();
    await user.click(within(slot).getByRole('button', { name: 'Search transcript' }));
    expect(within(slot).getByRole('searchbox', { name: 'Search transcript' })).toHaveFocus();
    slot.remove();
  });
});
