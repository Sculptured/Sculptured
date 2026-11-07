import { useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import { plural } from '../format';
import { Diff } from '../shell/ToolCard';
import { scrollWithin } from '../shell/highlight';
import { Icon } from '../shell/ui';
import { fileTree, type FileEntry, type TreeNode } from './process';
import { ACTION_ICON, STEP_WORD } from './visualParts';

/** File rows drawn before "Show all". */
const FILES = 200;

function fileName(path: string) {
  return path.split('/').pop() || path;
}

type TreeProps = { byPath: Map<string, FileEntry>; selected: string | null; onSelect: (path: string) => void };

function DirRow({ n, depth, ...rest }: { n: TreeNode; depth: number } & TreeProps) {
  return (
    <li className="vp-dir">
      <span className="vp-dir-name" style={{ paddingLeft: depth * 14 }} data-raw-evidence>
        <Icon name="folder" size={13} className="muted" />
        {n.name}/
      </span>
      <ul>
        <TreeRows nodes={n.children} depth={depth + 1} {...rest} />
      </ul>
    </li>
  );
}

function FileRow({ n, depth, byPath, selected, onSelect }: { n: TreeNode; depth: number } & TreeProps) {
  const f = byPath.get(n.path)!;
  const changed = f.changes.length > 0;
  return (
    <li>
      <button
        type="button"
        className={`vp-file a-${f.last.action}${changed ? ' changed' : ''}`}
        style={{ paddingLeft: 8 + depth * 14 }}
        aria-pressed={selected === n.path}
        onClick={() => onSelect(n.path)}
      >
        <Icon name={ACTION_ICON[f.last.action]} size={13} />
        <span className="vp-file-name" data-raw-evidence>
          {n.name}
        </span>
        <span className="vp-file-badges">
          {changed && <span className="vp-badge change">{plural(f.changes.length, 'change')}</span>}
          {f.reads > 0 && <span className="vp-badge read">read ×{f.reads}</span>}
        </span>
        <span className="vp-file-last">
          {f.last.action} · turn {f.last.phase}
        </span>
      </button>
    </li>
  );
}

function TreeRows({ nodes, depth, ...rest }: { nodes: TreeNode[]; depth: number } & TreeProps) {
  return <>{nodes.map((n) => (n.isFile ? <FileRow key={n.path} n={n} depth={depth} {...rest} /> : <DirRow key={`d-${n.path}`} n={n} depth={depth} {...rest} />))}</>;
}

/** Bring the changes into view when a file is picked, inside this tab's own scroller only. */
function useBringIntoView(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const el = ref.current;
    const box = el?.closest<HTMLElement>('.vp');
    if (!el || !box) return;
    const r = el.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    if (r.top > b.bottom - 80 || r.top < b.top) scrollWithin(box, el, 'start');
  }, [ref]);
}

function ChangeBlock({ c, i, onJump }: { c: FileEntry['changes'][number]; i: number; onJump: (key: string) => void }) {
  return (
    <div className="vp-change">
      <div className="vp-change-head">
        <span>
          Turn {c.phase} · {c.action}
        </span>
        <span className={`tstat t-${c.status}`}>{STEP_WORD[c.status]}</span>
        <span className="pane-spacer" />
        <button type="button" className="link-btn" onClick={() => onJump(c.key)}>
          Show in transcript
        </button>
      </div>
      <div className="tc-body" data-raw-evidence>
        {c.hunks.length === 0 ? <p className="muted">No change text in the call.</p> : c.hunks.map((h, j) => <Diff key={`${i}-${j}`} old={h.old} next={h.new} />)}
      </div>
    </div>
  );
}

function ReadsNote({ f, onJump }: { f: FileEntry; onJump: (key: string) => void }) {
  if (f.reads === 0) return null;
  return (
    <p className="vp-file-reads muted">
      Read {plural(f.reads, 'time')}.
      {f.last.action === 'read' && (
        <>
          {' '}
          <button type="button" className="link-btn" onClick={() => onJump(f.last.key)}>
            Show last read
          </button>
        </>
      )}
    </p>
  );
}

function FileDetail({ f, onJump, onClose }: { f: FileEntry; onJump: (key: string) => void; onClose: () => void }) {
  const ref = useRef<HTMLElement>(null);
  useBringIntoView(ref);
  return (
    <section ref={ref} className="vp-file-detail" aria-label={`Changes to ${fileName(f.path)}`}>
      <div className="vp-file-detail-head">
        <code data-raw-evidence>{f.path}</code>
        <button type="button" className="icon-btn" onClick={onClose} aria-label="Close file" data-tip="Close file">
          <Icon name="x" size={13} />
        </button>
      </div>
      {f.changes.length === 0 && <p className="muted">Read only: no changes seen.</p>}
      {f.changes.map((c, i) => <ChangeBlock key={c.key} c={c} i={i} onJump={onJump} />)}
      <ReadsNote f={f} onJump={onJump} />
    </section>
  );
}

function FilesBody({ files, selected, onSelect, onJump }: { files: FileEntry[]; selected: string | null; onSelect: (p: string | null) => void; onJump: (key: string) => void }) {
  const [all, setAll] = useState(false);
  const shown = all ? files : files.slice(0, FILES);
  const tree = useMemo(() => fileTree(shown.map((f) => f.path)), [shown]);
  const byPath = useMemo(() => new Map(files.map((f) => [f.path, f])), [files]);
  const sel = selected ? byPath.get(selected) : undefined;
  return (
    <>
      {tree.root && (
        <p className="vp-root muted" data-raw-evidence>
          in {tree.root}/
        </p>
      )}
      <ul className="vp-tree">
        <TreeRows nodes={tree.nodes} byPath={byPath} selected={selected} onSelect={(p) => onSelect(p === selected ? null : p)} depth={0} />
      </ul>
      {files.length > FILES && (
        <button type="button" className="link-btn" onClick={() => setAll((v) => !v)}>
          {all ? 'Show fewer' : `Show all ${files.length} files`}
        </button>
      )}
      {sel && <FileDetail key={sel.path} f={sel} onJump={onJump} onClose={() => onSelect(null)} />}
    </>
  );
}

export function Files({ files, selected, onSelect, onJump }: { files: FileEntry[]; selected: string | null; onSelect: (p: string | null) => void; onJump: (key: string) => void }) {
  const changed = files.filter((f) => f.changes.length > 0).length;
  return (
    <section className="vp-panel vp-files" aria-label="Built and changed">
      <div className="vp-panel-head">
        <h3>Built and changed</h3>
        <span className="vp-panel-count">
          {changed} changed · {files.length - changed} read only
        </span>
      </div>
      <p className="vp-note">
        <span className="tier-inline tier-observed">◆ observed</span> from the agent’s read, edit and write tool calls. Files changed by shell commands are not listed.
      </p>
      {files.length === 0 ? <p className="muted vp-empty">No files read or changed yet.</p> : <FilesBody files={files} selected={selected} onSelect={onSelect} onJump={onJump} />}
    </section>
  );
}
