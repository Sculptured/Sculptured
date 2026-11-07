import { BackToApp } from './BackToApp';
import { useCallback, useEffect, useState, type ComponentType, type FormEvent } from 'react';
import { gw } from './session';
import { AdminAnalytics } from './AdminAnalytics';
import { AdminAudit } from './AdminAudit';
import { AdminRunning } from './AdminRunning';
import { AdminUsers, type User } from './AdminUsers';
import { AdminGroupLinks } from './AdminGroupLinks';
import './hosted.css';

type Invite = { id: string; kind: string; role: string; name: string; email: string | null; note: string | null; created_at: string; expires_at: string; used_at: string | null; revoked_at: string | null; slug?: string | null; max_uses?: number | null; uses?: number };

type View = 'people' | 'running' | 'audit' | 'analytics';
const VIEWS: [View, string][] = [['people', 'People & invites'], ['running', 'Running now'], ['audit', 'Audit log'], ['analytics', 'Analytics']];

const open = (i: Invite) => new Date(i.expires_at) < new Date() ? 'expired' : `open until ${new Date(i.expires_at).toLocaleDateString()}`;
/** A group link counts who joined; a personal invite is used once. */
const groupState = (i: Invite) => `${i.uses ?? 0} joined${i.max_uses ? ` of ${i.max_uses}` : ''} · ${i.revoked_at ? 'revoked' : open(i)}`;
const state = (i: Invite) => i.slug ? groupState(i) : i.revoked_at ? 'revoked' : i.used_at ? `used ${new Date(i.used_at).toLocaleDateString()}` : open(i);

const BLANK_FORM = { name: '', email: '', note: '', days: 14, admin: false };
type InviteForm = typeof BLANK_FORM;
type Act = (fn: () => Promise<unknown>) => () => Promise<void>;

function InviteSection({ made, onMade, onError }: { made: string | null; onMade: (link: string) => void; onError: (message: string) => void }) {
  const [form, setForm] = useState<InviteForm>(BLANK_FORM);
  const create = async (e: FormEvent) => {
    e.preventDefault(); onError('');
    try {
      const r = await gw<{ link: string }>('POST', '/admin/invites', { ...form, email: form.email || null, note: form.note || null });
      onMade(r.link); setForm(BLANK_FORM);
    } catch (err) { onError((err as Error).message); }
  };
  return (
    <section aria-label="New invite">
      <h2>Invite someone</h2>
      <form onSubmit={create} className="hosted-form inline">
        <label>Name<input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} required maxLength={120} /></label>
        <label>Email (optional)<input type="email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} /></label>
        <label>Note (optional)<input value={form.note} onChange={(e) => setForm({ ...form, note: e.target.value })} maxLength={200} /></label>
        <label>Valid for (days)<input type="number" min={1} max={90} value={form.days} onChange={(e) => setForm({ ...form, days: Number(e.target.value) })} /></label>
        <label className="check"><input type="checkbox" checked={form.admin} onChange={(e) => setForm({ ...form, admin: e.target.checked })} /> Admin</label>
        <button className="btn btn-primary">Create invite link</button>
      </form>
      {made && <p className="hosted-link"><code>{made}</code> <button className="btn" onClick={() => void navigator.clipboard?.writeText(made)}>Copy</button> <span className="muted">This link is shown only now; send it to the person.</span></p>}
    </section>
  );
}

function InviteRow({ i, act }: { i: Invite; act: Act }) {
  return (
    <tr><td>{i.name}{i.slug ? ` · /join/${i.slug}` : ''}{i.email ? ` · ${i.email}` : ''}{i.role === 'admin' ? ' · admin' : ''}</td><td>{i.slug ? 'group link' : i.kind}</td><td>{state(i)}</td><td>{i.note}</td>
      <td>{(!i.used_at || i.slug) && !i.revoked_at && <button className="btn" onClick={act(() => gw('POST', `/admin/invites/${i.id}/revoke`))}>Revoke</button>}</td></tr>
  );
}

function InvitesTable({ invites, act }: { invites: Invite[]; act: Act }) {
  return (
    <section aria-label="Invites">
      <h2>Invites</h2>
      <table><thead><tr><th>For</th><th>Kind</th><th>Status</th><th>Note</th><th /></tr></thead><tbody>
        {invites.map((i) => <InviteRow key={i.id} i={i} act={act} />)}
      </tbody></table>
    </section>
  );
}

function useAdminData() {
  const [invites, setInvites] = useState<Invite[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [error, setError] = useState('');
  const load = useCallback(() => {
    gw<Invite[]>('GET', '/admin/invites').then(setInvites, (e) => setError(e.message));
    gw<User[]>('GET', '/admin/users').then(setUsers, (e) => setError(e.message));
  }, []);
  useEffect(load, [load]);
  const act: Act = (fn) => async () => { setError(''); try { await fn(); load(); } catch (e) { setError((e as Error).message); } };
  return { invites, users, error, setError, load, act };
}

function PeopleView({ data, made, setMade }: { data: ReturnType<typeof useAdminData>; made: string | null; setMade: (link: string | null) => void }) {
  const { invites, users, error, setError, load, act } = data;
  return <>
    {error && <p className="tone-red" role="alert">{error}</p>}
    <InviteSection made={made} onMade={(link) => { setMade(link); load(); }} onError={setError} />
    <AdminGroupLinks onMade={load} />
    <InvitesTable invites={invites} act={act} />
    <AdminUsers users={users} act={act} onLink={setMade} />
  </>;
}

const OTHER_VIEWS: Partial<Record<View, ComponentType>> = { analytics: AdminAnalytics, running: AdminRunning, audit: AdminAudit };

export function AdminPage(_: { csrf?: string }) {
  const data = useAdminData();
  const [made, setMade] = useState<string | null>(null);
  const [view, setView] = useState<View>('people');
  const Other = OTHER_VIEWS[view];
  return (
    <main className="hosted-page hosted-admin">
      <BackToApp />
      <div className="admin-head">
        <h1>Beta admin</h1>
        {/* The ops site signs admins in with a one-time ticket from here (opens in a new tab). */}
        <a className="btn" href="/gw/api/admin/ops-ticket?next=%2F" target="_blank" rel="noopener">Ops tools: databases and logs ↗</a>
      </div>
      <div role="tablist" aria-label="Admin" className="an-tabs">
        {VIEWS.map(([v, label]) => <button key={v} role="tab" aria-selected={view === v} className={`btn${view === v ? ' is-on' : ''}`} onClick={() => setView(v)}>{label}</button>)}
      </div>
      {Other ? <Other /> : <PeopleView data={data} made={made} setMade={setMade} />}
    </main>
  );
}
