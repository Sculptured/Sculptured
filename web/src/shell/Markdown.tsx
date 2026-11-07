import { marked, type Token, type Tokens } from 'marked';
import { memo, useEffect, useRef, useState, type ReactNode } from 'react';
import { Hl } from './highlight';
import { Icon } from './icons';

/**
 * Markdown for agent replies. `marked` only tokenises; every element here is made by React, so
 * raw HTML in a reply is never parsed into elements: it shows as the text it is, and scripts,
 * event handlers and styles cannot run. Links open in a new tab, and only web and mail links
 * become links. Images are not loaded; they show as a link to the image.
 */

const SAFE_HREF = /^(https?:\/\/|mailto:)/i;

function safeHref(href: string | null | undefined): string | null {
  const h = (href ?? '').trim();
  return SAFE_HREF.test(h) ? h : null;
}

/** Copies text, with a fallback for browsers without the async clipboard. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* fall through */
  }
  try {
    const t = document.createElement('textarea');
    t.value = text;
    t.setAttribute('readonly', '');
    t.style.position = 'fixed';
    t.style.opacity = '0';
    document.body.appendChild(t);
    t.select();
    const ok = document.execCommand?.('copy') ?? false;
    t.remove();
    return ok;
  } catch {
    return false;
  }
}

/** A copy button that says "Copied" for a moment after it worked. */
export function CopyButton({ text, label, className }: { text: string; label: string; className?: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => void (timer.current && clearTimeout(timer.current)), []);
  const copy = async () => {
    const ok = await copyText(text);
    setState(ok ? 'copied' : 'failed');
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setState('idle'), 1600);
  };
  return (
    <button type="button" className={`copy-btn${className ? ` ${className}` : ''}`} aria-label={label} data-tip={label} onClick={() => void copy()}>
      <Icon name={state === 'copied' ? 'check' : 'copy'} size={13} />
      <span aria-live="polite">{state === 'copied' ? 'Copied' : state === 'failed' ? 'Copy failed' : 'Copy'}</span>
    </button>
  );
}

function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const name = lang.trim().split(/\s+/)[0] || 'text';
  return (
    <figure className="md-code" aria-label={`${name} code`}>
      <figcaption className="md-code-bar">
        <span className="md-code-lang">{name}</span>
        <CopyButton text={text} label="Copy code" />
      </figcaption>
      <pre>
        <code>
          <Hl text={text} />
        </code>
      </pre>
    </figure>
  );
}

function inline(tokens: Token[] | undefined, key = 'i'): ReactNode[] {
  return (tokens ?? []).map((t, i) => inlineToken(t, `${key}.${i}`));
}

type InlineRender = (t: Token, key: string) => ReactNode;

const as = <T,>(f: (t: T, key: string) => ReactNode): InlineRender => (t, key) => f(t as T, key);

function textToken(t: Tokens.Text, key: string) {
  return t.tokens ? <span key={key}>{inline(t.tokens, key)}</span> : <Hl key={key} text={t.text} />;
}

function linkToken(l: Tokens.Link, key: string) {
  const href = safeHref(l.href);
  const body = inline(l.tokens, key);
  if (!href) return <span key={key}>{body}</span>;
  return (
    <a key={key} href={href} target="_blank" rel="noopener noreferrer" title={l.title ?? href} data-raw-evidence-title>
      {body}
    </a>
  );
}

function imageToken(im: Tokens.Image, key: string) {
  const href = safeHref(im.href);
  const label = `image: ${im.text || 'untitled'}`;
  return href ? (
    <a key={key} href={href} target="_blank" rel="noopener noreferrer">
      {label}
    </a>
  ) : (
    <span key={key}>{label}</span>
  );
}

const INLINE: Record<string, InlineRender> = {
  text: as(textToken),
  escape: as((t: Tokens.Escape, key) => <Hl key={key} text={t.text} />),
  strong: as((t: Tokens.Strong, key) => <strong key={key}>{inline(t.tokens, key)}</strong>),
  em: as((t: Tokens.Em, key) => <em key={key}>{inline(t.tokens, key)}</em>),
  del: as((t: Tokens.Del, key) => <del key={key}>{inline(t.tokens, key)}</del>),
  codespan: as((t: Tokens.Codespan, key) => (
    <code key={key} className="md-inline-code">
      <Hl text={t.text} />
    </code>
  )),
  br: (_t, key) => <br key={key} />,
  link: as(linkToken),
  image: as(imageToken),
  checkbox: as((t: Tokens.Checkbox, key) => <span key={key}>{t.checked ? '☑ ' : '☐ '}</span>),
};

function inlineToken(t: Token, key: string): ReactNode {
  // Raw HTML and anything unknown: shown as written.
  return (INLINE[t.type] ?? ((u, k) => <Hl key={k} text={(u as { raw: string }).raw} />))(t, key);
}

function blocks(tokens: Token[], key = 'b'): ReactNode[] {
  return tokens.map((t, i) => blockToken(t, `${key}.${i}`));
}

function listItem(item: Tokens.ListItem, key: string): ReactNode {
  const box = item.task ? <span className="md-task">{item.checked ? '☑ ' : '☐ '}</span> : null;
  // marked may already include a checkbox token; skip it so the box shows once.
  const toks = item.task ? item.tokens.filter((t) => t.type !== 'checkbox') : item.tokens;
  const content = toks.map((t, i) => (t.type === 'text' ? <span key={i}>{inline((t as Tokens.Text).tokens ?? [t], `${key}.${i}`)}</span> : blockToken(t, `${key}.${i}`)));
  return (
    <li key={key} className={item.task ? 'md-task-item' : undefined}>
      {box}
      {content}
    </li>
  );
}

const align = (a: string | null) => (a ? { textAlign: a as 'left' | 'right' | 'center' } : undefined);

function tableToken(tb: Tokens.Table, key: string) {
  return (
    <div key={key} className="md-table">
      <table>
        <thead>
          <tr>
            {tb.header.map((c, i) => (
              <th key={i} scope="col" style={align(tb.align[i])}>
                {inline(c.tokens, `${key}.h${i}`)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {tb.rows.map((row, r) => (
            <tr key={r}>
              {row.map((c, i) => (
                <td key={i} style={align(tb.align[i])}>
                  {inline(c.tokens, `${key}.${r}.${i}`)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function listToken(l: Tokens.List, key: string) {
  const items = l.items.map((it, i) => listItem(it, `${key}.${i}`));
  return l.ordered ? (
    <ol key={key} start={typeof l.start === 'number' && l.start !== 1 ? l.start : undefined}>
      {items}
    </ol>
  ) : (
    <ul key={key}>{items}</ul>
  );
}

const BLOCK: Record<string, InlineRender> = {
  space: () => null,
  def: () => null,
  heading: as((h: Tokens.Heading, key) => (
    <div key={key} role="heading" aria-level={Math.min(6, h.depth + 2)} className={`md-h md-h${h.depth}`}>
      {inline(h.tokens, key)}
    </div>
  )),
  paragraph: as((p: Tokens.Paragraph, key) => <p key={key}>{inline(p.tokens, key)}</p>),
  text: as((tt: Tokens.Text, key) => <p key={key}>{tt.tokens ? inline(tt.tokens, key) : <Hl text={tt.text} />}</p>),
  code: as((c: Tokens.Code, key) => <CodeBlock key={key} lang={c.lang ?? ''} text={c.text} />),
  blockquote: as((q: Tokens.Blockquote, key) => <blockquote key={key}>{blocks(q.tokens, key)}</blockquote>),
  hr: (_t, key) => <hr key={key} />,
  list: as(listToken),
  table: as(tableToken),
};

function rawBlock(t: Token, key: string) {
  return (
    <p key={key} className="md-raw">
      <Hl text={(t as { raw: string }).raw.replace(/\n+$/, '')} />
    </p>
  );
}

function blockToken(t: Token, key: string): ReactNode {
  return (BLOCK[t.type] ?? rawBlock)(t, key);
}

/** Agent Markdown as React elements. Memoised: replies do not change once written. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  let tokens: Token[];
  try {
    tokens = marked.lexer(text, { gfm: true });
  } catch {
    return <p className="md-raw">{text}</p>;
  }
  return <div className="md">{blocks(tokens)}</div>;
});
