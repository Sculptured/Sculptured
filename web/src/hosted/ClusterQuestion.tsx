import { useEffect, useState, type FormEvent } from 'react';
import type { Cluster } from './Clusters';

/** `text` with its http(s) addresses as links (a site's web or device-code sign-in). */
export function Linked({ text }: { text: string }) {
  const parts = text.split(/(https?:\/\/[^\s<>"')]+)/g);
  return <>{parts.map((p, i) => (/^https?:\/\//.test(p) ? <a key={i} href={p} target="_blank" rel="noreferrer">{p}</a> : p))}</>;
}

/** Seconds until `at` (ISO), counting down each second; null without a time. */
function useSecondsLeft(at: string | null | undefined): number | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => { const t = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(t); }, []);
  return at ? Math.max(0, Math.round((new Date(at).getTime() - now) / 1000)) : null;
}

const PUSHY = /duo|push|passcode or option/i;

/** The cluster's own sign-in question (MFA): every prompt of the round, shown or hidden as the site
 * asks, sent once, then cleared from the page. */
export function Question({ c, onAnswer }: { c: Cluster; onAnswer: (a: string[]) => void }) {
  const q = c.question!;
  const [answers, setAnswers] = useState<string[]>(() => q.prompts.map(() => ''));
  const left = useSecondsLeft(q.expires_at);
  const push = PUSHY.test([q.name, q.instructions, ...q.prompts.map((p) => p.text)].join(' '));
  const send = (e: FormEvent) => { e.preventDefault(); onAnswer(answers); setAnswers(q.prompts.map(() => '')); };
  return (
    <form className="hosted-form" onSubmit={send} aria-label={`${q.host} asks`}>
      <p><strong>{q.host} asks</strong></p>
      {q.name && <p><strong>{q.name}</strong></p>}
      {q.instructions && <p className="cluster-instructions"><Linked text={q.instructions} /></p>}
      {q.prompts.map((p, i) => (
        <label key={i}><Linked text={p.text.trim()} />
          <input type={p.echo ? 'text' : 'password'} autoComplete="one-time-code" value={answers[i] ?? ''} onChange={(e) => setAnswers((a) => a.map((x, j) => (j === i ? e.target.value : x)))} />
        </label>
      ))}
      {push && <p className="muted">For a push, type 1 (or push) and approve on your phone; Sculptured waits up to {q.wait_after_answer ?? 120} s for the approval.</p>}
      <p className="muted">Used once to sign in, never kept.{left !== null && ` ${left} s left to answer.`}</p>
      <button type="submit" className="btn btn-primary">Send answer</button>
    </form>
  );
}
