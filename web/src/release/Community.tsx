import { track } from '../hosted/analytics';
import {DismissibleError} from '../shell/DismissibleError';
import { useEffect, useState, useRef, useCallback } from "react";
import { Link, useLocation } from "react-router-dom";
import { BlockDetails, BlockDescription, BlockFeatures } from "../groups/BlockDetails";
import { blockTransferUrl } from "../groups/blockTransfer";
import { rememberWorkspace } from "./workspaceLink";
import { LocalCommunity } from "./LocalCommunity";
import type { GroupBlock } from "../generated/GroupBlock";
import type { Publication } from "../generated/Publication";
import type { CommunitySession } from "../generated/CommunitySession";
import { hosted } from "./Nav";
import { AgentDesignGuide } from "./AgentDesignGuide";
import { TemplatePreview } from "../structure/TemplateGallery";
import { ComponentBlockCard, type ComponentBlock } from "./ComponentBlockCard";
import { sanitizeExport, downloadBlock } from "./sharing";
export async function communityRequest<T>(
  path: string,
  session?: CommunitySession,
  method = "GET",
  body?: unknown,
): Promise<T> {
  const r = await fetch(`/api/community/${path}`, {
    method,
    headers: {
      "Content-Type": "application/json",
      ...(session?.csrf ? { "X-CSRF-Token": session.csrf } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await r.json();
  if (!r.ok) throw Error(data.error ?? "Request failed");
  return data;
}
export function useCommunitySession() {
  const [session, setSession] = useState<CommunitySession>({
      user: null,
      csrf: null,
    }),
    [error, setError] = useState("");
  useEffect(() => {
    if (hosted)
      communityRequest<CommunitySession>("me").then(setSession, (e) =>
        setError(String(e)),
      );
  }, []);
  return { session, setSession, error };
}
export function Community() {
  const { pathname } = useLocation();
  const tab = pathname.split("/")[2] ?? "explore";
  const { session, error: authError } = useCommunitySession();
  const [blocks, setBlocks] = useState<Publication[]>([]),
    [selected, setSelected] = useState<Publication | null>(null),
    [query, setQuery] = useState(""),
    [offset, setOffset] = useState(0),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [loading, setLoading] = useState(false);
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (!selected) return;
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.showModal();
    return () => {
      previous?.focus();
    };
  }, [selected]);
  const visibleBlocks =
    tab === "explore"
      ? blocks
      : blocks.filter((b) =>
          `${b.title} ${b.description} ${b.tags.join(" ")}`
            .toLowerCase()
            .includes(query.toLowerCase()),
        );
  const reload = useCallback(() => {
    setLoading(true);
    communityRequest<{ blocks: Publication[] }>(
      tab === "saved"
        ? "saved"
        : tab === "mine"
          ? "mine"
          : `blocks?q=${encodeURIComponent(query)}&offset=${offset}`,
    )
      .then(
        (d) => setBlocks(d.blocks),
        (e) => setError(String(e)),
      )
      .finally(() => setLoading(false));
  }, [tab, query, offset]);
  useEffect(() => {
    if (hosted && tab !== "publish") reload();
  }, [tab, reload, session.user?.id]);
  const action = async (path: string, method = "POST", body?: unknown) => {
    setError("");
    try {
      await communityRequest(path, session, method, body);
      setNotice("Saved.");
      reload();
    } catch (e) {
      setError(String(e));
    }
  };
  const addToProject=(block:GroupBlock)=>{
    try {track('add-from-community');const origin=rememberWorkspace();if(!origin)throw Error('Open Community from your local workspace to choose a project.');window.location.assign(blockTransferUrl(origin,block));}
    catch(e){setError((e as Error).message);}
  };
  if (tab === "learn") return <AgentDesignGuide />;
  if (!hosted) return <LocalCommunity />;
  return (
    <section
      className={`release-page ${(session.user?.settings as { compact?: boolean })?.compact !== false ? "community-compact" : ""}`}
    >
      <p className="eyebrow">Built by people. Composed by you.</p>
      <header className="release-heading">
        <div>
          <h1>
            {tab === "saved"
              ? "Your saved blocks"
              : tab === "mine"
                ? "Your publications"
                : tab === "publish"
                  ? "Publish a group"
                  : "Explore agent teams"}
          </h1>
          <p>Reusable roles, conversations, and collaboration patterns.</p>
        </div>
        {session.user ? (
          <Link className="btn" to="/community/publish">
            Publish a block
          </Link>
        ) : (
          <a className="btn btn-primary" href="/api/community/login">
            Sign in
          </a>
        )}
      </header>
      <nav className="release-tabs">
        <Link to="/community/explore">Explore</Link>
        <Link to="/community/saved">Saved</Link>
        <Link to="/community/mine">My publications</Link>
        <Link to="/community/learn">Learn</Link>
      </nav>
      {(error || authError) && <DismissibleError>{error || authError}</DismissibleError>}
      {notice && <p role="status">{notice}</p>}
      {tab === "publish" ? (
        <PublishForm
          session={session}
          onPublished={() => {
            setNotice("Published. Your block is now public.");
          }}
        />
      ) : (
        <>
          <form
            className="release-search"
            onSubmit={(e) => {
              e.preventDefault();
              setOffset(0);
              reload();
            }}
          >
            <input
              aria-label="Search blocks"
              placeholder="Search roles, systems, and ideas…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button className="btn">Search</button>
          </form>
          {loading && <p role="status">Loading blocks…</p>}
          {!loading && !visibleBlocks.length && (
            <div className="release-empty">
              <h2>
                {tab === "explore"
                  ? "A place for useful collaboration"
                  : "Nothing saved here yet"}
              </h2>
              <p>
                Publish a reviewed group or browse the catalog to save a
                version.
              </p>
            </div>
          )}
          <div className="release-grid">
            {visibleBlocks.map((b) => (b as { kind?: string }).kind === "component" ? <ComponentBlockCard key={b.id} block={b as unknown as ComponentBlock} /> : (
              <article className="release-card block-publication" key={b.id}>
                <button
                  className="publication-preview"
                  onClick={() => setSelected(b)}
                  aria-label={`Inspect ${b.title}`}
                >
                  <TemplatePreview draft={b.content.draft} practice={false} />
                </button>
                <div className="release-tags">
                  {b.tags.map((t) => (
                    <span key={t}>{t}</span>
                  ))}
                </div>
                <h2>
                  <button onClick={() => setSelected(b)}>{b.title}</button>
                </h2>
                <BlockDescription text={b.description.split("\n\n")[0]}/>
                <BlockFeatures block={b.content}/>
                <small>
                  By {b.author} · v{b.version} · {b.license}
                  {!b.published ? " · Unpublished" : ""}
                </small>
                <div className="release-actions">
                  <button className="btn btn-primary" onClick={()=>addToProject(b.content)}>Add to project</button>
                  <button className="btn" onClick={() => setSelected(b)}>
                    Inspect
                  </button>
                  <button
                    className="btn"
                    disabled={!session.user}
                    onClick={() =>
                      action(
                        `blocks/${b.id}/save`,
                        tab === "saved" ? "DELETE" : "POST",
                      )
                    }
                  >
                    {tab === "saved" ? "Remove save" : "Save block"}
                  </button>
                  {!session.user && <small>Sign in to save</small>}
                </div>
              </article>
            ))}
          </div>
          {tab === "explore" && (
            <div className="release-actions">
              <button
                className="btn"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - 24))}
              >
                Previous
              </button>
              <span>Page {1 + offset / 24}</span>
              <button
                className="btn"
                disabled={blocks.length < 24}
                onClick={() => setOffset(offset + 24)}
              >
                Next
              </button>
            </div>
          )}
        </>
      )}
      {selected && (
        <dialog
          ref={dialogRef}
          open={undefined}
          className="release-drawer block-modal block-inspect-modal"
          aria-label={selected.title}
          onCancel={() => setSelected(null)}
        >
          <header><div><h2>{selected.title}</h2><p className="muted">By {selected.author} · v{selected.version} · {selected.license}</p></div><button className="btn" onClick={() => setSelected(null)}>Close</button></header>
          <BlockDescription text={selected.description}/>
          <BlockFeatures block={selected.content}/>
          <div className="release-actions"><button className="btn btn-primary" onClick={()=>addToProject(selected.content)}>Add to project</button><button className="btn" onClick={()=>downloadBlock(selected.content)}>Download group</button></div>
          <BlockDetails key={selected.id} block={selected.content}/>
          <p className="muted">Community supplied instructions · publication is not verification. Review before starting agents.</p>
          <details>
            <summary>Complete group & fingerprint</summary>
            <code>{selected.digest}</code>
            <pre>{JSON.stringify(selected.content, null, 2)}</pre>
          </details>
          {session.user?.id === selected.owner && (
            <button
              className="btn"
              onClick={() => {
                void action(`blocks/${selected.id}`, "DELETE");
                setSelected(null);
              }}
            >
              Unpublish
            </button>
          )}
          {session.user && (
            <details><summary>Report a concern</summary><form
              onSubmit={(e) => {
                e.preventDefault();
                const data = new FormData(e.currentTarget);
                void action(`blocks/${selected.id}/report`, "POST", {
                  reason: data.get("reason"),
                });
              }}
            >
              <label>
                Report a concern
                <textarea
                  name="reason"
                  minLength={5}
                  maxLength={1000}
                  required
                />
              </label>
              <button className="btn">Send report</button>
            </form></details>
          )}
        </dialog>
      )}
    </section>
  );
}
function PublishForm({
  session,
  onPublished,
}: {
  session: CommunitySession;
  onPublished: () => void;
}) {
  const [block, setBlock] = useState<GroupBlock | null>(null),
    [reviewed, setReviewed] = useState(false),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  if (!session.user)
    return (
      <p>
        <a href="/api/community/login">Sign in</a> before publishing.
      </p>
    );
  return (
    <form
      className="release-card release-form"
      onSubmit={async (e) => {
        e.preventDefault();
        if (!block) return;
        setBusy(true);
        setError("");
        const f = new FormData(e.currentTarget);
        try {
          await communityRequest("blocks", session, "POST", {
            title: f.get("title"),
            description: f.get("description"),
            license: f.get("license"),
            tags: String(f.get("tags"))
              .split(",")
              .map((s) => s.trim())
              .filter(Boolean),
            content: block,
            reviewed,
          });
          setBlock(null);
          setReviewed(false);
          onPublished();
        } catch (e) {
          setError(String(e));
        } finally {
          setBusy(false);
        }
      }}
    >
      <label>
        Group export
        <input
          type="file"
          accept=".json,application/json"
          onChange={async (e) => {
            setReviewed(false);
            setError("");
            try {
              const f = e.target.files?.[0];
              if (!f) return;
              if (f.size > 500000) throw Error("Group must be under 500 KB");
              setBlock(sanitizeExport(JSON.parse(await f.text())));
            } catch (e) {
              setBlock(null);
              setError(String(e));
            }
          }}
        />
      </label>
      <label>
        Title
        <input name="title" required maxLength={150} />
      </label>
      <label>
        Description
        <textarea name="description" required maxLength={3000} />
      </label>
      <label>
        Tags, separated by commas
        <input name="tags" maxLength={240} />
      </label>
      <label>
        License
        <select name="license">
          <option>MIT</option>
          <option>Apache-2.0</option>
          <option>CC0-1.0</option>
        </select>
      </label>
      {block && (
        <details open>
          <summary>Review exactly what will be published</summary>
          <pre>{JSON.stringify(block, null, 2)}</pre>
        </details>
      )}
      <label className="check-label">
        <input
          type="checkbox"
          checked={reviewed}
          onChange={(e) => setReviewed(e.target.checked)}
          required
        />
        I reviewed every prompt for private information, own the rights to share
        this group, and choose to publish it under this license.
      </label>
      <p>
        Local paths, model overrides, run inputs, and external connection notes
        are removed. Prompt text can still contain secrets—review it.
      </p>
      {error && <DismissibleError>{error}</DismissibleError>}
      <button
        className="btn btn-primary"
        disabled={!block || !reviewed || busy}
      >
        {busy ? "Publishing…" : "Publish publicly"}
      </button>
    </form>
  );
}
