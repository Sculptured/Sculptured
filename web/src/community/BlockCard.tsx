import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { Publication } from '../generated/community/Publication';
import { TemplatePreview } from '../structure/TemplateGallery';
import { AuthorChip } from './AuthorChip';
import { KIND_LABEL, whereItRuns } from '../structure/components';
import { componentOf, opensHere } from './kinds';

/** A component: what it gives agents and exactly what it runs. */
function ComponentSummary({ pub }: { pub: Publication }) {
  const c = componentOf(pub);
  if (!c) return null;
  return (
    <div className="cm-component">
      <p>{c.description}</p>
      <p className="muted">{KIND_LABEL[c.kind]} · <code>{whereItRuns(c)}</code>{c.install && <> · set up: {c.install}</>}</p>
    </div>
  );
}

/** One block in a grid: its shape, title, tags, author, version and licence. */
export function BlockCard({ pub, actions }: { pub: Publication; actions?: ReactNode }) {
  return (
    <article className="cm-card">
      {opensHere(pub) && <div className="cm-preview"><TemplatePreview draft={pub.content.draft} /></div>}
      <ComponentSummary pub={pub} />
      <h3><Link to={`/community/blocks/${encodeURIComponent(pub.id)}`}>{pub.title}</Link></h3>
      {pub.tags.length > 0 && <ul className="cm-tags" aria-label="Tags">{pub.tags.map((t) => <li key={t}>{t}</li>)}</ul>}
      <p className="cm-meta"><AuthorChip author={pub.author} /> <span className="muted">v{pub.version} · {pub.license}</span></p>
      {actions}
    </article>
  );
}
