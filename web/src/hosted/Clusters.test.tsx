import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { Clusters, type Cluster } from './Clusters';

afterEach(() => vi.restoreAllMocks());
const reply = (status: number, body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status }));

const base: Cluster = {
  id: 'c1', name: 'Perlmutter', host: 'perlmutter.nersc.gov', port: 22, username: 'alex', jump: null, auth: 'generated',
  public_key: 'ssh-ed25519 AAAAPUB sculptured@app.example.com',
  authorized_keys: { line: 'restrict ssh-ed25519 AAAAPUB sculptured@app.example.com', command: "mkdir -p ~/.ssh && echo 'restrict ssh-ed25519 AAAAPUB sculptured@app.example.com' >> ~/.ssh/authorized_keys", jump_line: null },
  saved: { key: true, password: false }, host_key: null, jump_host_key: null, state: 'new', detail: '', busy: false, step: null, question: null,
  needs_mfa: false, keeper: null, installed: null, doctor: null, doctor_at: null, started_at: null, last_attempt_at: null, failures: 0,
  created_at: '2026-10-20T10:00:00Z', device_id: null, connected: false, events: [],
};

type Call = { method: string; url: string; body: unknown };
function serve(clusters: () => Cluster[], extra: (c: Call) => Promise<Response> | null = () => null) {
  const calls: Call[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const c = { method: init?.method ?? 'GET', url: String(url), body: init?.body ? JSON.parse(String(init.body)) : undefined };
    calls.push(c);
    const r = extra(c);
    if (r) return r;
    if (c.url.endsWith('/gw/api/clusters') && c.method === 'GET') return reply(200, { clusters: clusters(), available: true });
    return reply(404, {});
  });
  return calls;
}

it('adds a cluster with a key Sculptured makes and shows the exact authorized_keys line, after the login-node policy note', async () => {
  let list: Cluster[] = [];
  const calls = serve(() => list, (c) => {
    if (c.url.endsWith('/gw/api/clusters') && c.method === 'POST') { list = [base]; return reply(200, base); }
    return null;
  });
  render(<Clusters />);
  await userEvent.click(await screen.findByRole('button', { name: 'Add a cluster' }));
  expect(screen.getByText(/check your site's rules on long-running programs on login nodes/i)).toBeInTheDocument();
  await userEvent.type(screen.getByLabelText('Login node'), 'perlmutter.nersc.gov');
  await userEvent.type(screen.getByLabelText('User name'), 'alex');
  await userEvent.click(screen.getByRole('button', { name: 'Add cluster' }));
  const card = await screen.findByRole('article', { name: 'Perlmutter' });
  expect(within(card).getByText('restrict ssh-ed25519 AAAAPUB sculptured@app.example.com')).toBeInTheDocument();
  expect(within(card).getByText(/Add this line to ~\/\.ssh\/authorized_keys on perlmutter\.nersc\.gov/)).toBeInTheDocument();
  const post = calls.find((c) => c.method === 'POST' && c.url.endsWith('/gw/api/clusters'));
  expect(post?.body).toMatchObject({ host: 'perlmutter.nersc.gov', username: 'alex', port: 22, auth: 'generated' });
  expect(calls.some((c) => c.url.includes('/connect')), 'waits for the user to add the key').toBe(false);
  await userEvent.click(within(card).getByRole('button', { name: 'Connect Perlmutter' }));
  await waitFor(() => expect(calls.some((c) => c.method === 'POST' && c.url.endsWith('/gw/api/clusters/c1/connect'))).toBe(true));
});

it('a pasted password is sent once, then the cluster connects at once', async () => {
  let list: Cluster[] = [];
  const pw = { ...base, auth: 'password' as const, public_key: null, authorized_keys: null, saved: { key: false, password: true } };
  const calls = serve(() => list, (c) => {
    if (c.url.endsWith('/gw/api/clusters') && c.method === 'POST') { list = [pw]; return reply(200, pw); }
    if (c.url.endsWith('/connect')) return reply(200, { ok: true });
    return null;
  });
  render(<Clusters />);
  await userEvent.click(await screen.findByRole('button', { name: 'Add a cluster' }));
  await userEvent.type(screen.getByLabelText('Login node'), 'login.example.edu');
  await userEvent.type(screen.getByLabelText('User name'), 'alex');
  await userEvent.click(screen.getByRole('radio', { name: /A password/ }));
  const field = screen.getByLabelText('Password');
  expect(field).toHaveAttribute('type', 'password');
  await userEvent.type(field, 's3cret');
  await userEvent.click(screen.getByRole('button', { name: 'Add cluster' }));
  await waitFor(() => expect(calls.some((c) => c.url.endsWith('/gw/api/clusters/c1/connect'))).toBe(true));
  expect(calls.find((c) => c.method === 'POST' && c.url.endsWith('/gw/api/clusters'))?.body).toMatchObject({ auth: 'password', password: 's3cret' });
  expect(screen.queryByDisplayValue('s3cret')).toBeNull();
  expect(await screen.findByText(/Password saved \(encrypted\)/)).toBeInTheDocument();
});

it('shows the cluster’s sign-in question and sends the answer once, hidden as it is typed', async () => {
  const asking: Cluster = { ...base, state: 'needs_answer', busy: true, step: 'Waiting for your answer', question: { host: 'perlmutter.nersc.gov', name: '', instructions: '', prompts: [{ text: 'Duo passcode: ', echo: false }] } };
  const calls = serve(() => [asking], (c) => (c.url.endsWith('/answer') ? reply(200, { ok: true }) : null));
  render(<Clusters />);
  const box = await screen.findByLabelText('Duo passcode:');
  expect(box).toHaveAttribute('type', 'password');
  expect(screen.getByText(/perlmutter\.nersc\.gov asks/)).toBeInTheDocument();
  await userEvent.type(box, '123456');
  await userEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await waitFor(() => expect(calls.find((c) => c.url.endsWith('/gw/api/clusters/c1/answer'))?.body).toEqual({ answers: ['123456'] }));
  expect(screen.queryByDisplayValue('123456')).toBeNull();
});

it('lists the cluster’s audit log, its doctor report and states such as “Reconnect needs your code”', async () => {
  const c: Cluster = {
    ...base, state: 'needs_code', detail: 'Reconnect needs your code.', needs_mfa: true, keeper: 'tmux', host_key: 'SHA256:abc', device_id: 'd1', doctor: 'Claude Code: not installed',
    events: [
      { at: '2026-10-20T10:05:00Z', action: 'needs your code', ok: false, detail: 'The cluster asked a question at sign-in.' },
      { at: '2026-10-20T10:01:00Z', action: 'started', ok: true, detail: 'Started the runner in the tmux session ‘sculptured-runner’.' },
      { at: '2026-10-20T10:00:00Z', action: 'signed in', ok: true, detail: 'Signed in to alex@perlmutter.nersc.gov with key + your answer.' },
    ],
  };
  serve(() => [c]);
  render(<Clusters />);
  const card = await screen.findByRole('article', { name: 'Perlmutter' });
  expect(within(card).getByText('Reconnect needs your code')).toBeInTheDocument();
  expect(within(card).getByRole('button', { name: 'Reconnect Perlmutter' })).toBeInTheDocument();
  const log = within(card).getByRole('list', { name: 'What Sculptured did on Perlmutter' });
  const items = within(log).getAllByRole('listitem');
  expect(items).toHaveLength(3);
  expect(items[0]).toHaveTextContent(/needs your code/);
  expect(items[0]).toHaveTextContent(/failed|problem/i);
  expect(items[2]).toHaveTextContent(/key \+ your answer/);
  expect(within(card).getByText('Claude Code: not installed')).toBeInTheDocument();
  expect(within(card).getByText(/SHA256:abc/)).toBeInTheDocument();
});

it('remove asks first and says the saved key is deleted', async () => {
  let removed = false;
  const calls = serve(() => (removed ? [] : [{ ...base, state: 'running', connected: true, device_id: 'd1' }]), (c) => {
    if (c.url.endsWith('/remove')) { removed = true; return reply(200, { ok: true }); }
    return null;
  });
  render(<Clusters />);
  const card = await screen.findByRole('article', { name: 'Perlmutter' });
  expect(within(card).getByText('Connected')).toBeInTheDocument();
  await userEvent.click(within(card).getByRole('button', { name: 'Remove Perlmutter' }));
  expect(screen.getByText(/deletes its saved key or password and disconnects its runner/)).toBeInTheDocument();
  expect(calls.some((c) => c.url.endsWith('/remove'))).toBe(false);
  await userEvent.click(screen.getByRole('button', { name: 'Yes, remove Perlmutter' }));
  await waitFor(() => expect(screen.queryByRole('article', { name: 'Perlmutter' })).toBeNull());
});

it('adds a certificate cluster, keeps it signed in 12 hours by default, and says what a web app cannot do', async () => {
  let list: Cluster[] = [];
  const calls = serve(() => list, (c) => {
    if (c.url.endsWith('/gw/api/clusters') && c.method === 'POST') { list = [{ ...base, auth: 'certificate', authorized_keys: null }]; return reply(200, list[0]); }
    if (c.url.endsWith('/connect')) return reply(200, { ok: true });
    return null;
  });
  render(<Clusters />);
  await userEvent.click(await screen.findByRole('button', { name: 'Add a cluster' }));
  expect(screen.getByText(/hardware security keys/i)).toBeInTheDocument();
  expect(screen.getByText(/Kerberos/)).toBeInTheDocument();
  expect(screen.getByText(/agent forwarding/i)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Set up a cluster yourself' })).toHaveAttribute('href', 'https://docs.usesculptured.com/connect-a-computer#set-up-a-cluster-yourself');
  expect(screen.getByLabelText('Keep signed in for (hours)')).toHaveValue(12);
  expect(screen.getByText(/many sites limit how long a session/i)).toBeInTheDocument();
  await userEvent.type(screen.getByLabelText('Login node'), 'perlmutter.nersc.gov');
  await userEvent.type(screen.getByLabelText('User name'), 'alex');
  await userEvent.click(screen.getByRole('radio', { name: /A certificate/ }));
  await userEvent.type(screen.getByLabelText('Private key'), 'KEY');
  await userEvent.type(screen.getByLabelText('Certificate (-cert.pub)'), 'ssh-ed25519-cert-v01@openssh.com AAAA');
  await userEvent.click(screen.getByRole('button', { name: 'Add cluster' }));
  await waitFor(() => expect(calls.some((c) => c.url.endsWith('/gw/api/clusters/c1/connect'))).toBe(true));
  expect(calls.find((c) => c.method === 'POST' && c.url.endsWith('/gw/api/clusters'))?.body).toMatchObject({ auth: 'certificate', private_key: 'KEY', certificate: 'ssh-ed25519-cert-v01@openssh.com AAAA', hold_hours: 12 });
});

it('shows how long the cluster stays signed in, and Sign out closes it', async () => {
  const calls = serve(() => [{ ...base, state: 'running', connected: true, held_until: '2026-10-21T00:00:00Z', hold_hours: 12 }], (c) => (c.url.endsWith('/sign-out') ? reply(200, { ok: true }) : null));
  render(<Clusters />);
  const card = await screen.findByRole('article', { name: 'Perlmutter' });
  expect(within(card).getByText(/Signed in until/)).toBeInTheDocument();
  await userEvent.click(within(card).getByRole('button', { name: 'Sign out of Perlmutter' }));
  await waitFor(() => expect(calls.some((c) => c.method === 'POST' && c.url.endsWith('/gw/api/clusters/c1/sign-out'))).toBe(true));
});

it('an expired certificate asks for a new one, pasted on the card', async () => {
  const cert: Cluster = { ...base, auth: 'certificate', authorized_keys: null, state: 'failed', detail: 'Certificate expired — paste a new one on the cluster’s card.',
    certificate: { principals: ['alex'], key_id: 'sshproxy', valid_before: '2026-10-19T00:00:00Z', warning: 'expired' } };
  const calls = serve(() => [cert], (c) => (c.url.endsWith('/certificate') ? reply(200, cert) : null));
  render(<Clusters />);
  const card = await screen.findByRole('article', { name: 'Perlmutter' });
  expect(within(card).getByText(/Certificate expired — paste a new one/)).toBeInTheDocument();
  expect(within(card).getByText(/for alex/)).toBeInTheDocument();
  await userEvent.type(within(card).getByLabelText('New certificate (-cert.pub)'), 'ssh-ed25519-cert-v01@openssh.com NEW');
  await userEvent.type(within(card).getByLabelText('Its private key (if it changed)'), 'NEWKEY');
  await userEvent.click(within(card).getByRole('button', { name: 'Save certificate' }));
  await waitFor(() => expect(calls.find((c) => c.url.endsWith('/gw/api/clusters/c1/certificate'))?.body).toEqual({ certificate: 'ssh-ed25519-cert-v01@openssh.com NEW', private_key: 'NEWKEY' }));
});

it('a round with two prompts shows its name and instructions, links a sign-in URL, respects echo and counts down for a push', async () => {
  const expires = new Date(Date.now() + 120_000).toISOString();
  const asking: Cluster = { ...base, state: 'needs_answer', busy: true, step: 'Waiting for your answer', question: {
    host: 'login.example.edu', name: 'Duo two-factor login', instructions: 'Or approve at https://duo.example.org/approve', expires_at: expires, wait_after_answer: 120,
    prompts: [{ text: 'Badge number: ', echo: true }, { text: 'Passcode or option (1-3): ', echo: false }] } };
  const calls = serve(() => [asking], (c) => (c.url.endsWith('/answer') ? reply(200, { ok: true }) : null));
  render(<Clusters />);
  expect(await screen.findByText('Duo two-factor login')).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'https://duo.example.org/approve' })).toHaveAttribute('href', 'https://duo.example.org/approve');
  expect(screen.getByLabelText('Badge number:')).toHaveAttribute('type', 'text');
  expect(screen.getByLabelText('Passcode or option (1-3):')).toHaveAttribute('type', 'password');
  expect(screen.getByText(/type 1 \(or push\) and approve on your phone/i)).toBeInTheDocument();
  expect(screen.getByText(/waits up to 120 s/)).toBeInTheDocument();
  expect(screen.getByText(/s left to answer/)).toBeInTheDocument();
  await userEvent.type(screen.getByLabelText('Badge number:'), '4711');
  await userEvent.type(screen.getByLabelText('Passcode or option (1-3):'), 'hunter2 123456');
  await userEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  await waitFor(() => expect(calls.find((c) => c.url.endsWith('/answer'))?.body).toEqual({ answers: ['4711', 'hunter2 123456'] }));
});
