import {DismissibleError} from '../shell/DismissibleError';
import { useEffect, useState, useCallback } from "react";
import { Link } from "react-router-dom";
import { hosted } from "./Nav";
import { communityRequest, useCommunitySession } from "./Community";
export function Profile() {
  const { session, setSession, error } = useCommunitySession();
  const [name, setName] = useState(""),
    [compact, setCompact] = useState(true),
    [notice, setNotice] = useState("");
  useEffect(() => {
    setName(session.user?.name ?? "");
    setCompact(
      (session.user?.settings as { compact?: boolean })?.compact ?? true,
    );
  }, [session.user]);
  if (!hosted)
    return (
      <section className="release-page community-account">
        <h1>Community</h1>
        <div className="release-card">
          <h2>Local workspace</h2>
          <p>
            This runner belongs to your operating-system account. Codex and
            Claude use their own local CLI sign-in.
          </p>
          <p>
            Community identity is separate. Signing in to Community grants no
            access to this machine, project folders, or transcripts.
          </p>
          <Link className="btn btn-primary" to="/community">
            Open community account
          </Link>
        </div>
      </section>
    );
  return (
    <section className="release-page community-account">
      <h1>Community</h1>
      {!session.user ? (
        <a className="btn btn-primary" href="/api/community/login">
          Sign in securely
        </a>
      ) : (
        <>
          <form
            className="release-card release-form"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await communityRequest("me", session, "PATCH", {
                  name,
                  settings: { compact },
                });
                setSession(await communityRequest("me"));
                setNotice("Profile saved in the database.");
              } catch (e) {
                setNotice(String(e));
              }
            }}
          >
            <label>
              Public display name
              <input
                required
                maxLength={80}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label className="check-label">
              <input
                type="checkbox"
                checked={compact}
                onChange={(e) => setCompact(e.target.checked)}
              />
              Compact community cards
            </label>
            <p>
              Your display name appears on publications. Credentials and
              multifactor authentication are managed by the sign-in provider.
            </p>
            <button className="btn">Save profile</button>
          </form>
          <div className="release-card">
            <h2>Sessions</h2>
            <p>
              Sessions expire after 12 hours, or one hour of inactivity.
              Provider tokens are never stored in browser storage.
            </p>
            <div className="release-actions">
              <button
                className="btn"
                onClick={async () => {
                  try {
                    await communityRequest("logout", session, "POST", {});
                    setSession({ user: null, csrf: null });
                  } catch (e) {
                    setNotice(String(e));
                  }
                }}
              >
                Sign out
              </button>
              <button
                className="btn"
                onClick={async () => {
                  try {
                    await communityRequest(
                      "sessions/revoke",
                      session,
                      "POST",
                      {},
                    );
                    setSession({ user: null, csrf: null });
                    setNotice("All community sessions revoked.");
                  } catch (e) {
                    setNotice(String(e));
                  }
                }}
              >
                Sign out all devices
              </button>
            </div>
          </div>
          {session.user.admin && <Moderation session={session} />}
        </>
      )}
      <p role="status">{error || notice}</p>
    </section>
  );
}
function Moderation({
  session,
}: {
  session: import("../generated/CommunitySession").CommunitySession;
}) {
  const [reports, setReports] = useState<
      { id: string; block: string; reason: string }[]
    >([]),
    [error, setError] = useState("");
  const reload = useCallback(
    () =>
      communityRequest<{ reports: typeof reports }>("reports", session).then(
        (d) => setReports(d.reports),
        (e) => setError(String(e)),
      ),
    [session],
  );
  useEffect(() => {
    void reload();
  }, [reload]);
  return (
    <div className="release-card">
      <h2>Moderation queue</h2>
      {reports.map((r) => (
        <article key={r.id}>
          <p>{r.reason}</p>
          <code>{r.block}</code>
          <button
            className="btn"
            onClick={async () => {
              try {
                await communityRequest(
                  `blocks/${r.block}/moderate`,
                  session,
                  "POST",
                  { hidden: true },
                );
                await reload();
              } catch (e) {
                setError(String(e));
              }
            }}
          >
            Hide publication
          </button>
          <button
            className="btn"
            onClick={async () => {
              try {
                await communityRequest(
                  `blocks/${r.block}/moderate`,
                  session,
                  "POST",
                  { hidden: false },
                );
                await reload();
              } catch (e) {
                setError(String(e));
              }
            }}
          >
            Resolve without hiding
          </button>
        </article>
      ))}
      {!reports.length && <p>No open reports.</p>}
      {error && <DismissibleError>{error}</DismissibleError>}
    </div>
  );
}
