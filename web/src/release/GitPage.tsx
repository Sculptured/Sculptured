import { DismissibleError } from '../shell/DismissibleError';
import { useEffect, useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { api } from '../api';
import type { GitSnapshot } from '../generated/GitSnapshot';
import { AgentCopiesList } from './AgentCopiesList';

type Git = ReturnType<typeof useGit>;

/** The project's Git state and the actions on it; every action replaces the snapshot with the server's. */
function useGit(team: string) {
  const [data, setData] = useState<GitSnapshot | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [patch, setPatch] = useState('');
  const [message, setMessage] = useState('');
  const reload = () => {
    setError('');
    api.gitStatus(team).then(setData, (e) => setError(e.message));
  };
  useEffect(reload, [team]);
  async function action(name: string, text = '') {
    if (!data) return;
    setBusy(true);
    setError('');
    try {
      setData(await api.gitAction(team, name, data.fingerprint, selected, text));
      setSelected([]);
      setPatch('');
      if (name === 'commit') setMessage('');
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  const showDiff = (path: string, staged: boolean, empty: string) =>
    api.gitDiff(team, path, staged).then((d) => setPatch(d.text || empty), (e) => setError(e.message));
  const toggle = (path: string, on: boolean) => setSelected((a) => (on ? [...a, path] : a.filter((p) => p !== path)));
  return { data, setData, error, busy, selected, patch, message, setMessage, reload, action, showDiff, toggle };
}

function FileRow({ f, git }: { f: GitSnapshot['files'][number]; git: Git }) {
  return (
    <div>
      <label>
        <input type="checkbox" checked={git.selected.includes(f.path)} onChange={(e) => git.toggle(f.path, e.target.checked)} />
        <code>{f.index}{f.worktree}</code>{' '}
        {f.path}
      </label>
      <button className="btn" onClick={() => git.showDiff(f.path, false, 'No tracked unstaged diff. New files are not included until staged.')}>Diff</button>
      <button className="btn" onClick={() => git.showDiff(f.path, true, 'No staged diff.')}>Staged diff</button>
    </div>
  );
}

function ChangesCard({ data, git }: { data: GitSnapshot; git: Git }) {
  const none = git.busy || !git.selected.length;
  return (
    <div className="release-card">
      <h2>{data.branch}</h2>
      <code>{data.head || 'No commits yet'}</code>
      <p>Index = staged changes; worktree = unstaged changes. Status refreshes on request. Agents and external Git clients can change files concurrently.</p>
      <div className="git-files">{data.files.map((f) => <FileRow key={f.path} f={f} git={git} />)}</div>
      {!data.files.length && <p>Working tree clean.</p>}
      <div className="release-actions">
        <button className="btn" disabled={none} onClick={() => git.action('stage')}>Stage selected</button>
        <button className="btn" disabled={none} onClick={() => git.action('unstage')}>Unstage selected</button>
      </div>
    </div>
  );
}

function CommitForm({ data, git }: { data: GitSnapshot; git: Git }) {
  return (
    <form className="release-card release-form" onSubmit={(e) => { e.preventDefault(); void git.action('commit', git.message); }}>
      <h2>Commit staged changes</h2>
      <label>
        Commit message
        <textarea required value={git.message} onChange={(e) => git.setMessage(e.target.value)} maxLength={2000} />
      </label>
      <p>Commits the whole index, including files staged elsewhere. Review the staged diff first. Hooks and signing are disabled for UI commits; use your terminal if your project requires them.</p>
      <button disabled={git.busy || !data.files.some((f) => f.index !== ' ' && f.index !== '?')} className="btn btn-primary">Commit staged changes</button>
    </form>
  );
}

function BranchForm({ data, git }: { data: GitSnapshot; git: Git }) {
  const [branch, setBranch] = useState('');
  return (
    <form className="release-card release-form" onSubmit={(e) => { e.preventDefault(); void git.action('branch', branch); }}>
      <h2>Create a branch</h2>
      <label>
        New branch name
        <input value={branch} onChange={(e) => setBranch(e.target.value)} required maxLength={100} />
      </label>
      <button className="btn" disabled={git.busy}>Create and switch</button>
      <h2>Recent commits</h2>
      <pre>{data.log || 'No commits yet.'}</pre>
    </form>
  );
}

function Repository({ data, git }: { data: GitSnapshot; git: Git }) {
  return (
    <>
      <ChangesCard data={data} git={git} />
      {git.patch && <pre className="release-card git-diff" tabIndex={0}>{git.patch}</pre>}
      <div className="release-grid">
        <CommitForm data={data} git={git} />
        <BranchForm data={data} git={git} />
      </div>
    </>
  );
}

function NoRepository({ git }: { git: Git }) {
  return (
    <div className="release-card">
      <h2>No repository in this project folder</h2>
      <button disabled={git.busy} className="btn" onClick={() => git.action('init')}>Initialize repository</button>
    </div>
  );
}

export function GitPage() {
  const { team = '' } = useParams();
  const git = useGit(team);
  const { data } = git;
  return (
    <section className="release-page">
      <header className="release-heading">
        <div>
          <h1>Git</h1>
          <p>Inspect changes and commit deliberately. Nothing is pushed automatically.</p>
        </div>
        <Link className="btn" to={`/team/${team}/structure`}>Back to board</Link>
      </header>
      {git.error && <DismissibleError>{git.error}</DismissibleError>}
      <button className="btn" onClick={git.reload}>Refresh status</button>
      {data && (data.repository ? <Repository data={data} git={git} /> : <NoRepository git={git} />)}
      <AgentCopiesList team={team} onMerged={() => api.gitStatus(team).then(git.setData, () => {})} />
    </section>
  );
}
