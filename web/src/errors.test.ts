import { describe, expect, it } from 'vitest';
import { ApiError } from './api';
import { ERROR_WORDS, errorText, STARTING_REASON } from './errors';

const raw = 'SERVER-TEXT-THAT-MUST-NOT-SHOW';

describe('errorText: server errors in plain words', () => {
  it.each(['bad_json', 'bad_body', 'json_required', 'body_too_large', 'bad_query', 'bad_url', 'method_not_allowed'])('maps the envelope code %s and never shows the server text', (code) => {
    const text = errorText(new ApiError(400, code, `${raw} expected value at line 1 column 1`));
    expect(text).toBe(ERROR_WORDS[code]);
    expect(text).not.toContain(raw);
  });

  it('says what to do for each envelope code', () => {
    expect(errorText(new ApiError(400, 'bad_json', raw))).toMatch(/Reload the page/);
    expect(errorText(new ApiError(422, 'bad_body', raw))).toMatch(/Reload the page/);
    expect(errorText(new ApiError(415, 'json_required', raw))).toMatch(/Reload the page/);
    expect(errorText(new ApiError(413, 'body_too_large', raw))).toMatch(/too large/);
    expect(errorText(new ApiError(400, 'bad_query', raw))).toMatch(/Reload the page/);
  });

  it('a stop refused while the team starts reads like the disabled Stop button', () => {
    expect(errorText(new ApiError(409, 'team_starting', raw))).toBe(STARTING_REASON);
    expect(STARTING_REASON).toBe('Starting… you can stop the team once it has started');
  });

  it('a name taken by another team does not suggest renaming, since a team id never changes', () => {
    const text = errorText(new ApiError(409, 'rig_name_taken', raw));
    expect(text).not.toMatch(/pick another name|rename/i);
    expect(text).toBe(
      'Another team already uses this ID, and a team’s ID never changes. If the other team was started from a file, change the name inside that file and start it again.',
    );
  });

  it('a request from the wrong address says what to do, in plain words', () => {
    const text = errorText(new ApiError(403, 'forbidden_host', "requests must use this machine's sculptured address"));
    expect(text).toBe(
      'Sculptured only answers pages opened at its own address on this computer. Open Sculptured from the address it printed when it started (for example http://127.0.0.1:4410).',
    );
    expect(text).not.toContain('sculptured address');
  });

  it('a refused start counts the problems it carries', () => {
    const issue = { element: { kind: 'agent' as const, id: 'a' }, severity: 'error' as const, code: 'x', message: 'm', detail: null, spec_path: null };
    expect(errorText(new ApiError(422, 'team_invalid', raw, undefined, [issue]))).toBe('1 thing to fix');
    expect(errorText(new ApiError(422, 'team_invalid', raw, undefined, [issue, issue]))).toBe('2 things to fix');
  });

  it('keeps a host reason verbatim, as runtime evidence', () => {
    expect(errorText(new ApiError(502, 'host_unreachable', "Sculptured's agent runner is not connected"))).toBe(
      "Sculptured could not reach this agent's host: Sculptured's agent runner is not connected",
    );
  });

  it("keeps an agent's refusal of an answer verbatim, as runtime evidence", () => {
    expect(errorText(new ApiError(409, 'answer_refused', 'unsupported: an answer to a question this agent is not asking'))).toBe(
      'The agent did not take this answer: unsupported: an answer to a question this agent is not asking',
    );
  });

  it('never shows the text of an unknown code, a non-JSON answer or an internal error', () => {
    for (const e of [new ApiError(418, 'something_new', raw), new ApiError(502, 'http_error', `<html>${raw}</html>`), new ApiError(500, 'internal', raw)]) {
      expect(errorText(e)).not.toContain(raw);
    }
    expect(errorText(new ApiError(418, 'something_new', raw))).toBe('Sculptured could not do that (error 418). Reload the page and try again.');
  });

  it('a failed fetch says Sculptured could not be reached', () => {
    expect(errorText(new TypeError('Failed to fetch'))).toBe('Could not reach Sculptured. Check that it is still running, then try again.');
  });

  it('uses no OpenRig words in any mapped sentence', () => {
    const openRig = /\b(rigs?|pods?|seats?|members?|edges?|runtimes?|rigspec|yaml|agent_ref|profiles?)\b/i;
    for (const words of Object.values(ERROR_WORDS)) expect(words.replace(/Sculptured/g, '')).not.toMatch(openRig);
  });

  it('has plain words for every M2 error code', () => {
    for (const code of ['question_closed', 'bad_answer', 'question_open', 'reason_required', 'fork_unsupported', 'agent_not_running', 'no_restore_needed', 'bad_settings', 'team_not_running', 'restore_failed', 'restart_failed', 'cancel_timed_out', 'restore_pending', 'question_busy', 'answer_refused', 'answer_confirmed'])
      expect(ERROR_WORDS[code], code).toMatch(/\w/);
  });

  it("gives a start refusal in the server's own words, which name the computer, plan or project", () => {
    expect(errorText(new ApiError(409, 'needs_computer', 'Choose which computer runs this team: Design → Run on.'))).toBe('Choose which computer runs this team: Design → Run on.');
    expect(errorText(new ApiError(409, 'plan_limit', 'Codex plan is at 100% of its weekly limit.'))).toBe('Codex plan is at 100% of its weekly limit.');
    expect(errorText(new ApiError(409, 'needs_computer', ''))).not.toMatch(/^$/);
  });

  it('says how to update an older agent runner', () => {
    const words = errorText(new ApiError(409, 'older_runner', 'server text is not shown'));
    expect(words).toContain('older version');
    expect(words).toContain('Stop all teams');
    expect(words).not.toContain('server text');
  });
});
