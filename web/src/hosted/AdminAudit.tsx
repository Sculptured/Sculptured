import { useEffect, useState } from 'react';
import { gw } from './session';

type Entry = { id: number; at: string; action: string; detail: Record<string, unknown>; admin_email: string | null; target_email: string | null };

const PHRASES: Record<string, (who: string, e: Entry) => string> = {
  'invite.created': (_, e) => (e.detail.admin ? 'created an admin invite' : 'created an invite'),
  'invite.revoked': () => 'revoked an invite',
  'user.reset_link_made': (who) => `made a reset link for ${who}`,
  'user.disabled': (who) => `disabled ${who}`,
  'user.enabled': (who) => `enabled ${who}`,
  'user.admin_granted': (who) => `made ${who} an admin`,
  'user.admin_removed': (who) => `removed ${who}'s admin rights`,
  'user.signed_out': (who) => `signed ${who} out everywhere`,
  'device.revoked': (who) => `revoked a computer of ${who}`,
  'instance.stopped': (who) => `stopped ${who}'s Sculptured`,
  'ops.signed_in': () => 'opened the ops tools',
};

/** Every admin action in plain words. Unknown actions show their code word rather than a guess. */
function words(e: Entry): string {
  return PHRASES[e.action]?.(e.target_email ?? 'a user', e) ?? e.action;
}

export function AdminAudit() {
  const [list, setList] = useState<Entry[]>([]);
  const [error, setError] = useState('');
  useEffect(() => { gw<Entry[]>('GET', '/admin/audit').then(setList, (e) => setError((e as Error).message)); }, []);
  return (
    <section aria-label="Audit log">
      <h2>Audit log</h2>
      <p className="muted">Every admin action, newest first (the latest 500).</p>
      {error && <p className="tone-red" role="alert">{error}</p>}
      <table aria-label="Admin actions"><thead><tr><th>When</th><th>Admin</th><th>What</th></tr></thead><tbody>
        {list.map((e) => <tr key={e.id}><td>{new Date(e.at).toLocaleString()}</td><td>{e.admin_email ?? 'a removed admin'}</td><td>{words(e)}</td></tr>)}
      </tbody></table>
    </section>
  );
}
