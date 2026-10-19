import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { Publication } from '../generated/community/Publication';
import { TemplatePreview } from '../structure/TemplateGallery';
import { AuthorChip } from './AuthorChip';
import { opensHere } from './kinds';

/** One block in a grid: its shape, title, tags, author, version and licence. */
export function BlockCard({ pub, actions }: { pub: Publication; actions?: ReactNode }) {
  return (
    <article className="cm-card">
      {opensHere(pub) && <div className="cm-preview"><TemplatePreview draft={pub.content.draft} practice={false} /></div>}
      <h3><Link to={`/community/blocks/${encodeURIComponent(pub.id)}`}>{pub.title}</Link></h3>
      {pub.tags.length > 0 && <ul className="cm-tags" aria-label="Tags">{pub.tags.map((t) => <li key={t}>{t}</li>)}</ul>}
      <p className="cm-meta"><AuthorChip author={pub.author} /> <span className="muted">v{pub.version} · {pub.license}</span></p>
      {actions}
    </article>
  );
}
