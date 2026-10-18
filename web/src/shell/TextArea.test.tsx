import { useState } from 'react';
import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { TextArea } from './TextArea';

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});
function Editor({ submit = vi.fn() }: { submit?: () => void }) {
  const [value, setValue] = useState('Original');
  return <form onSubmit={e => { e.preventDefault(); submit(); }}>
    <label htmlFor="job">Job</label>
    <TextArea id="job" value={value} maxLength={100} onChange={e => setValue(e.target.value)} />
  </form>;
}
it('edits the same value full screen and returns focus without submitting', async () => {
  const submit = vi.fn(), user = userEvent.setup();
  render(<Editor submit={submit} />);
  await user.click(screen.getByRole('button', { name: 'Expand editor' }));
  const dialog = screen.getByRole('dialog', { name: 'Expanded editor' });
  const field = within(dialog).getByRole('textbox', { name: 'Job' });
  expect(field).toHaveFocus();
  expect(field).toHaveAttribute('maxlength', '100');
  await user.clear(field);
  await user.paste('A long pasted instruction');
  await user.keyboard('{Enter}Another line');
  await user.click(within(dialog).getByRole('button', { name: 'Done' }));
  expect(screen.getByLabelText('Job')).toHaveValue('A long pasted instruction\nAnother line');
  expect(screen.getByRole('button', { name: 'Expand editor' })).toHaveFocus();
  expect(submit).not.toHaveBeenCalled();
});
it('Escape keeps edits and read-only or disabled behavior is preserved', async () => {
  render(<Editor />);
  fireEvent.click(screen.getByRole('button', { name: 'Expand editor' }));
  const dialog = screen.getByRole('dialog');
  fireEvent.change(within(dialog).getByRole('textbox'), { target: { value: 'Kept' } });
  fireEvent(dialog, new Event('cancel', { cancelable: true }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(screen.getByLabelText('Job')).toHaveValue('Kept');
});
