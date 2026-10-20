import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import { SkillCatalog } from './SkillCatalog';
import * as catalog from './githubSkills';

beforeAll(() => { HTMLDialogElement.prototype.showModal ??= function () { this.setAttribute('open', ''); }; });
afterEach(() => vi.restoreAllMocks());
const skill = (name: string, description: string) => ({ path: `skills/${name}/SKILL.md`, skill: { name, description, body: `# ${name}` } });

it('lists a catalog, filters it, previews a skill and adds it', async () => {
  const load = vi.spyOn(catalog, 'loadCatalog').mockResolvedValue({ repo: 'google/skills', commit: 'abc1234def', skills: [skill('bigquery', 'Use when querying BigQuery.'), skill('cloud-run', 'Use when deploying to Cloud Run.')], unreadable: 0 });
  const onAdd = vi.fn();
  render(<SkillCatalog onAdd={onAdd} onClose={() => {}} />);
  const user = userEvent.setup();
  await user.selectOptions(screen.getByLabelText('Catalog'), 'google/skills');
  expect(load).toHaveBeenLastCalledWith('google/skills');
  expect(await screen.findByText('bigquery')).toBeInTheDocument();
  expect(screen.getByText(/google\/skills at abc1234/)).toBeInTheDocument();
  await user.type(screen.getByLabelText('Search skills'), 'run');
  expect(screen.queryByText('bigquery')).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Preview cloud-run' }));
  expect(screen.getByText('# cloud-run')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Add cloud-run' }));
  expect(onAdd).toHaveBeenCalledWith({ name: 'cloud-run', description: 'Use when deploying to Cloud Run.', body: '# cloud-run' });
});

it('reads another GitHub repository, and shows why one cannot be read', async () => {
  const load = vi.spyOn(catalog, 'loadCatalog').mockRejectedValue(new Error('GitHub has no repository a/b'));
  render(<SkillCatalog onAdd={() => {}} onClose={() => {}} />);
  const user = userEvent.setup();
  await user.selectOptions(screen.getByLabelText('Catalog'), 'other');
  await user.type(screen.getByLabelText('GitHub repository'), 'https://github.com/a/b');
  await user.click(screen.getByRole('button', { name: 'Open' }));
  expect(load).toHaveBeenLastCalledWith('a/b');
  await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('GitHub has no repository a/b'));
});
