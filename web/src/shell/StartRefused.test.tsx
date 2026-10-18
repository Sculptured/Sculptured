import { beforeAll, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { StartRefused } from './StartRefused';

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});

describe('A refused start', () => {
  it('pops up a plan hold with Start anyway', () => {
    const anyway = vi.fn(), close = vi.fn();
    render(<StartRefused refusal={{ text: 'Codex plan is at 100% of its weekly limit.', code: 'plan_limit' }} onClose={close} onAnyway={anyway} />);
    expect(screen.getByRole('alertdialog', { name: 'Your plan is near its limit' })).toBeTruthy();
    expect(screen.getByText(/Codex plan is at 100%/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Start anyway' }));
    expect(anyway).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
  });

  it('pops up any other reason with its issues and no override', () => {
    render(<StartRefused refusal={{ text: 'Choose which computer runs this team.', code: 'needs_computer', issues: [{ code: 'x', severity: 'error', message: 'Folder missing' } as never] }} onClose={() => {}} onAnyway={() => {}} />);
    expect(screen.getByRole('alertdialog', { name: 'The team could not start' })).toBeTruthy();
    expect(screen.getByText('Folder missing')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Start anyway' })).toBeNull();
  });
});
