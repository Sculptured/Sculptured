import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import { StartFromFile } from './StartFromFile';
import { openRigWords } from '../test-vocab';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, listRigs: vi.fn(), boot: vi.fn(), listTeams: vi.fn().mockResolvedValue([]) } };
});

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}
function renderAt(url: string) {
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/" element={<StartFromFile />} />
        <Route path="/p/:team/board" element={<p>rig page</p>} />
      </Routes>
      <Where />
    </MemoryRouter>,
  );
}

describe('Start a team from a file (New project · Advanced)', () => {
  beforeEach(() => vi.mocked(api.listRigs).mockReset());

  it('shows spec diagnostics from a 422', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockRejectedValue(
      new ApiError(422, 'spec_invalid', 'spec has errors', [
        { path: 'pods[0].members[0].runtime', severity: 'error', message: 'unknown runtime "gpt"' },
      ]),
    );
    renderAt('/');
    const user = userEvent.setup();
    expect(await screen.findByText('Enter a team file path to start.')).toBeInTheDocument();
    await user.type(await screen.findByLabelText('Team file'), '/tmp/bad.yaml');
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    const problems = await screen.findByRole('list', { name: 'Team file problems' });
    expect(problems).toHaveTextContent('pods[0].members[0].runtime — unknown runtime "gpt"');
    expect(api.boot).toHaveBeenCalledWith('/tmp/bad.yaml', expect.any(String));
  });

  // Ruling 2: team-file diagnostics carry spec words, so the expert form lives under Advanced.
  it('keeps the expert team-file form, its warnings and its problems under Advanced; a 422 shows no spec words outside it', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockRejectedValue(
      new ApiError(422, 'spec_invalid', 'the rig spec has errors', [
        { path: 'pods[0].members[0].runtime', severity: 'error', message: 'unknown runtime "gpt" for member a in pod dev' },
      ]),
    );
    renderAt('/');
    const user = userEvent.setup();
    const form = await screen.findByRole('form', { name: 'Start a team from a file' });
    const advanced = form.closest('details');
    expect(advanced).toHaveAttribute('data-advanced');
    expect(advanced).not.toHaveAttribute('open');
    await user.click(within(advanced!).getByText('Advanced'));
    expect(advanced).toHaveAttribute('open');
    await user.type(within(form).getByLabelText('Team file'), '/tmp/bad.yaml');
    await user.click(within(form).getByRole('button', { name: 'Start team' }));
    const alert = await screen.findByRole('alert');
    expect(advanced).toContainElement(alert);
    expect(within(alert).getByRole('list', { name: 'Team file problems' })).toHaveTextContent('unknown runtime "gpt"');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('a started file team with warnings shows them under Advanced', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockResolvedValue({
      rig: 'mini',
      diagnostics: [{ path: 'permission_policy', severity: 'warning', message: 'permission_policy is recorded but not enforced for this runtime' }],
    });
    renderAt('/');
    const user = userEvent.setup();
    const form = await screen.findByRole('form', { name: 'Start a team from a file' });
    await user.click(within(form.closest('details')!).getByText('Advanced'));
    await user.type(within(form).getByLabelText('Team file'), '/tmp/mini.yaml');
    await user.click(within(form).getByRole('button', { name: 'Start team' }));
    const notes = await screen.findByRole('status', { name: 'Start warnings' });
    expect(form.closest('details')).toContainElement(notes);
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('navigates to the booted rig', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockResolvedValue({ rig: 'mini', diagnostics: [] });
    renderAt('/');
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Team file'), '/tmp/mini.yaml');
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    expect(await screen.findByText('rig page')).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/p/mini/board');
  });

  // I4: a successful boot's warnings are shown until the operator navigates.
  it('shows the warnings of a successful boot and waits for the operator to open the rig', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockResolvedValue({
      rig: 'mini',
      diagnostics: [
        { path: 'permission_policy', severity: 'warning', message: 'permission_policy is recorded but not enforced until M2' },
        { path: 'culture_file', severity: 'not-enforced', message: 'declared · not enforced in this version' },
      ],
    });
    renderAt('/');
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Team file'), '/tmp/mini.yaml');
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    const notes = await screen.findByRole('status', { name: 'Start warnings' });
    expect(notes).toHaveTextContent('Started mini. The team file has a warning:');
    expect(notes).toHaveTextContent('warning · permission_policy — permission_policy is recorded but not enforced until M2');
    expect(notes).toHaveTextContent('declared · not enforced · culture_file');
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/$/);
    await user.click(screen.getByRole('link', { name: 'Open mini' }));
    expect(await screen.findByText('rig page')).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/p/mini/board');
  });

  it('navigates straight to the rig when the boot has only not-enforced notes', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot).mockResolvedValue({
      rig: 'mini',
      diagnostics: [{ path: 'culture_file', severity: 'not-enforced', message: 'declared · not enforced in this version' }],
    });
    renderAt('/');
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Team file'), '/tmp/mini.yaml');
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    expect(await screen.findByText('rig page')).toBeInTheDocument();
  });

  it('reuses the boot request id after a network failure and renews it after a server answer', async () => {
    vi.mocked(api.listRigs).mockResolvedValue([]);
    vi.mocked(api.boot)
      .mockReset()
      .mockRejectedValueOnce(new TypeError('network'))
      .mockRejectedValueOnce(new ApiError(422, 'spec_invalid', 'bad', []))
      .mockRejectedValueOnce(new TypeError('network'));
    renderAt('/');
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Team file'), '/tmp/x.yaml');
    const click = async () => {
      await user.click(screen.getByRole('button', { name: 'Start team' }));
      await screen.findByRole('alert');
    };
    await click();
    await click();
    await click();
    await click();
    const ids = vi.mocked(api.boot).mock.calls.map((c) => c[1]);
    expect(ids[0]).toBeTruthy();
    expect(ids[1]).toBe(ids[0]);
    expect(ids[2]).not.toBe(ids[1]);
  });
});
