import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from './api';

function mockFetch(status: number, body: unknown) {
  const fn = vi.fn().mockImplementation(async () => new Response(JSON.stringify(body), { status }));
  vi.stubGlobal('fetch', fn);
  return fn;
}

afterEach(() => vi.unstubAllGlobals());

describe('api', () => {
  it('posts send with request_id and an encoded seat id', async () => {
    const fetch = mockFetch(200, { message_id: 'm1' });
    const r = await api.send('dev.a@r', { text: 'hi', intent: 'instruct', interrupt: true }, 'req-1');
    expect(r.message_id).toBe('m1');
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe('/api/seats/dev.a%40r/send');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body)).toEqual({ request_id: 'req-1', text: 'hi', intent: 'instruct', interrupt: true });
  });

  it('turns a 422 into ApiError with diagnostics', async () => {
    mockFetch(422, {
      error: {
        code: 'spec_invalid',
        message: 'spec has errors',
        diagnostics: [{ path: 'pods[0].members[0].runtime', severity: 'error', message: 'unknown runtime "gpt"' }],
      },
    });
    const err = await api.boot('/tmp/x.yaml', 'req-2').catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(422);
    expect(err.code).toBe('spec_invalid');
    expect(err.diagnostics[0].path).toBe('pods[0].members[0].runtime');
  });

  it('pauses and resumes a whole team with request ids', async () => {
    const result = { affected: ['team.a@t'], skipped: [{ seat: 'team.b@t', code: 'not_running', reason: 'It is not running (stopped)' }] };
    const fetch = mockFetch(200, result);
    expect(await api.pauseTeam('t 1', 'req-p')).toEqual(result);
    await api.resumeTeam('t 1', 'req-r');
    const [[pauseUrl, pause], [resumeUrl, resume]] = fetch.mock.calls;
    expect([pauseUrl, pause.method, JSON.parse(pause.body)]).toEqual(['/api/teams/t%201/pause', 'POST', { request_id: 'req-p' }]);
    expect([resumeUrl, resume.method, JSON.parse(resume.body)]).toEqual(['/api/teams/t%201/resume', 'POST', { request_id: 'req-r' }]);
  });

  it('pauses one agent through its control', async () => {
    const answer = { mode: 'pause', affected: ['dev.a@r'], skipped: [] };
    const fetch = mockFetch(200, answer);
    expect(await api.controlAgent('dev.a@r', 'pause', 'req-c')).toEqual(answer);
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe('/api/seats/dev.a%40r/control');
    expect(JSON.parse(init.body)).toEqual({ request_id: 'req-c', mode: 'pause' });
  });

  it('labels a team resume with a new request id when none is given', async () => {
    const fetch = mockFetch(200, { affected: [], skipped: [] });
    await api.resumeTeam('t');
    expect(JSON.parse(fetch.mock.calls[0][1].body).request_id).toMatch(/.{8,}/);
  });

  it('generates a request id when none is given', async () => {
    const fetch = mockFetch(200, {});
    await api.interrupt('dev.a@r');
    expect(JSON.parse(fetch.mock.calls[0][1].body).request_id).toMatch(/.{8,}/);
  });

  it('reads the newest seat events with tail', async () => {
    const fetch = mockFetch(200, []);
    await api.latestSeatEvents('dev.a@r');
    expect(fetch.mock.calls[0][0]).toBe('/api/seats/dev.a%40r/events?tail=500');
  });

  it('turns a plain-text error body into an ApiError', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('Failed to parse the request body', { status: 400 })));
    const err = await api.listRigs().catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(400);
    expect(err.code).toBe('http_error');
    expect(err.message).toBe('Failed to parse the request body');
  });

  it('turns an HTML 200 body into an ApiError', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<!doctype html><html></html>', { status: 200 })));
    const err = await api.listRigs().catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.code).toBe('http_error');
  });

  it('turns a null JSON body into an ApiError', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('null', { status: 200 })));
    const err = await api.listRigs().catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.code).toBe('http_error');
  });
});

describe('team api', () => {
  it('saves a draft with PUT, the revision and a request id', async () => {
    const fetch = mockFetch(200, {});
    const draft = { id: 't', name: 'T', groups: [], agents: [], connections: [], revision: 3 };
    await api.saveTeam('t', 3, draft, 'req-9');
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe('/api/teams/t');
    expect(init.method).toBe('PUT');
    expect(JSON.parse(init.body)).toEqual({ request_id: 'req-9', revision: 3, draft });
  });

  it('carries plain-language issues on a 422', async () => {
    mockFetch(422, {
      error: {
        code: 'team_invalid',
        message: '1 thing to fix',
        issues: [{ element: { kind: 'agent', id: 'r' }, severity: 'error', code: 'folder-missing', message: 'Reviewer needs a folder to work in', detail: null, spec_path: null }],
      },
    });
    const err = await api.startTeam('t', 1, 'req-1').catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.code).toBe('team_invalid');
    expect(err.issues[0].message).toBe('Reviewer needs a folder to work in');
  });

  it('starts and stops a team with a request id and the revision', async () => {
    const fetch = mockFetch(200, { rig: 't', warnings: [] });
    await api.startTeam('t', 4, 'req-s');
    await api.stopTeam('t', 'req-x');
    expect(fetch.mock.calls[0][0]).toBe('/api/teams/t/start');
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ request_id: 'req-s', revision: 4 });
    expect(fetch.mock.calls[1][0]).toBe('/api/teams/t/stop');
    expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({ request_id: 'req-x' });
  });

  it('lists folders with an encoded path, or home without one, and creates one', async () => {
    const fetch = mockFetch(200, { path: '/h', parent: null, home: '/h', entries: [], truncated: false });
    await api.listFolders('/h/my dir');
    await api.listFolders();
    await api.createFolder('/h', 'new', 'req-f');
    expect(fetch.mock.calls[0][0]).toBe('/api/folders?path=%2Fh%2Fmy%20dir');
    expect(fetch.mock.calls[1][0]).toBe('/api/folders');
    expect(JSON.parse(fetch.mock.calls[2][1].body)).toEqual({ request_id: 'req-f', parent: '/h', name: 'new' });
  });

  it('creates a team from a template, and imports one', async () => {
    const fetch = mockFetch(200, {});
    await api.createTeam('blank', null, 'req-c');
    await api.importTeam('name: x', 'req-i');
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ request_id: 'req-c', template: 'blank', name: null });
    expect(fetch.mock.calls[1][0]).toBe('/api/teams/import');
    expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({ request_id: 'req-i', yaml: 'name: x' });
  });
});

describe('M2 client', () => {
  it('answers a question with a tagged answer and a request id', async () => {
    const fetch = mockFetch(200, { id: 'q1' });
    await api.answer('q/1', { kind: 'text', text: 'Green' }, 'req-a');
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe('/api/questions/q%2F1/answer');
    expect(JSON.parse(init.body)).toEqual({ request_id: 'req-a', answer: { kind: 'text', text: 'Green' } });
  });

  it('sends a recorded answer again with a request id', async () => {
    const fetch = mockFetch(200, { id: 'q1' });
    await api.sendAgain('q/1', 'req-g');
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe('/api/questions/q%2F1/send-again');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body)).toEqual({ request_id: 'req-g' });
  });

  it('puts settings and team settings', async () => {
    const fetch = mockFetch(200, {});
    await api.saveSettings({ posture: 'restricted', question_timeout_min: 10 }, 'req-s');
    await api.saveTeamSettings('my team', { posture: null, question_timeout_min: 3, own_copies: false, host: null }, 'req-t');
    expect(fetch.mock.calls[0][0]).toBe('/api/settings');
    expect(fetch.mock.calls[0][1].method).toBe('PUT');
    expect(fetch.mock.calls[1][0]).toBe('/api/teams/my%20team/settings');
    expect(JSON.parse(fetch.mock.calls[1][1].body).settings.question_timeout_min).toBe(3);
  });

  it('restores one agent and a whole team', async () => {
    const fetch = mockFetch(200, {});
    await api.restore('team.a@t', 'fresh', 'req-r');
    await api.restoreTeam('t', 'resume', 'req-rt');
    expect(fetch.mock.calls[0][0]).toBe('/api/seats/team.a%40t/restore');
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ request_id: 'req-r', decision: 'fresh' });
    expect(fetch.mock.calls[1][0]).toBe('/api/teams/t/restore');
    expect(fetch.mock.calls[1][1].method).toBe('POST');
    expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({ request_id: 'req-rt', decision: 'resume' });
  });
});

it('asks for a run\'s report of where tokens went, the latest run unless one is named', async () => {
  const fetch = mockFetch(200, {});
  await api.teamReport('c172');
  await api.teamReport('c172', 20);
  expect(fetch.mock.calls.map((c) => c[0])).toEqual(['/api/teams/c172/report', '/api/teams/c172/report?start=20']);
});

