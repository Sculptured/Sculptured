import {DismissibleError} from '../shell/DismissibleError';
import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { api } from "../api";
import type { GitSnapshot } from "../generated/GitSnapshot";
import { AgentCopiesList } from "./AgentCopiesList";
export function GitPage() {
  const { team = "" } = useParams();
  const [data, setData] = useState<GitSnapshot | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState<string[]>([]),
    [patch, setPatch] = useState(""),
    [message, setMessage] = useState(""),
    [branch, setBranch] = useState("");
  const reload = () => {
    setError("");
    api.gitStatus(team).then(setData, (e) => setError(e.message));
  };
  useEffect(reload, [team]);
  async function action(name: string, text = "") {
    if (!data) return;
    setBusy(true);
    setError("");
    try {
      setData(
        await api.gitAction(team, name, data.fingerprint, selected, text),
      );
      setSelected([]);
      setPatch("");
      if (name === "commit") setMessage("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="release-page">
      <header className="release-heading">
        <div>
          <h1>Git</h1>
          <p>
            Inspect changes and commit deliberately. Nothing is pushed
            automatically.
          </p>
        </div>
        <Link className="btn" to={`/team/${team}/structure`}>
          Back to board
        </Link>
      </header>
      {error && <DismissibleError>{error}</DismissibleError>}
      <button className="btn" onClick={reload}>
        Refresh status
      </button>
      {data && !data.repository ? (
        <div className="release-card">
          <h2>No repository in this project folder</h2>
          <button
            disabled={busy}
            className="btn"
            onClick={() => action("init")}
          >
            Initialize repository
          </button>
        </div>
      ) : (
        data && (
          <>
            <div className="release-card">
              <h2>{data.branch}</h2>
              <code>{data.head || "No commits yet"}</code>
              <p>
                Index = staged changes; worktree = unstaged changes. Status
                refreshes on request. Agents and external Git clients can change
                files concurrently.
              </p>
              <div className="git-files">
                {data.files.map((f) => (
                  <div key={f.path}>
                    <label>
                      <input
                        type="checkbox"
                        checked={selected.includes(f.path)}
                        onChange={(e) =>
                          setSelected((a) =>
                            e.target.checked
                              ? [...a, f.path]
                              : a.filter((p) => p !== f.path),
                          )
                        }
                      />
                      <code>
                        {f.index}
                        {f.worktree}
                      </code>{" "}
                      {f.path}
                    </label>
                    <button
                      className="btn"
                      onClick={() =>
                        api.gitDiff(team, f.path, false).then(
                          (d) =>
                            setPatch(
                              d.text ||
                                "No tracked unstaged diff. New files are not included until staged.",
                            ),
                          (e) => setError(e.message),
                        )
                      }
                    >
                      Diff
                    </button>
                    <button
                      className="btn"
                      onClick={() =>
                        api.gitDiff(team, f.path, true).then(
                          (d) => setPatch(d.text || "No staged diff."),
                          (e) => setError(e.message),
                        )
                      }
                    >
                      Staged diff
                    </button>
                  </div>
                ))}
              </div>
              {!data.files.length && <p>Working tree clean.</p>}
              <div className="release-actions">
                <button
                  className="btn"
                  disabled={busy || !selected.length}
                  onClick={() => action("stage")}
                >
                  Stage selected
                </button>
                <button
                  className="btn"
                  disabled={busy || !selected.length}
                  onClick={() => action("unstage")}
                >
                  Unstage selected
                </button>
              </div>
            </div>
            {patch && (
              <pre className="release-card git-diff" tabIndex={0}>
                {patch}
              </pre>
            )}
            <div className="release-grid">
              <form
                className="release-card release-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  void action("commit", message);
                }}
              >
                <h2>Commit staged changes</h2>
                <label>
                  Commit message
                  <textarea
                    required
                    value={message}
                    onChange={(e) => setMessage(e.target.value)}
                    maxLength={2000}
                  />
                </label>
                <p>
                  Commits the whole index, including files staged elsewhere.
                  Review the staged diff first. Hooks and signing are disabled
                  for UI commits; use your terminal if your project requires
                  them.
                </p>
                <button
                  disabled={
                    busy ||
                    !data.files.some((f) => f.index !== " " && f.index !== "?")
                  }
                  className="btn btn-primary"
                >
                  Commit staged changes
                </button>
              </form>
              <form
                className="release-card release-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  void action("branch", branch);
                }}
              >
                <h2>Create a branch</h2>
                <label>
                  New branch name
                  <input
                    value={branch}
                    onChange={(e) => setBranch(e.target.value)}
                    required
                    maxLength={100}
                  />
                </label>
                <button className="btn" disabled={busy}>
                  Create and switch
                </button>
                <h2>Recent commits</h2>
                <pre>{data.log || "No commits yet."}</pre>
              </form>
            </div>
          </>
        )
      )}
      <AgentCopiesList team={team} onMerged={() => api.gitStatus(team).then(setData, () => {})} />
    </section>
  );
}
