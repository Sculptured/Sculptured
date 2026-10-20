import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { gw } from './session';
import './hosted.css';

/** A pairing code's device: `scope` and `label` come from gateways that know Community access keys. */
type Device = { name: string; platform: string; user_code: string; requested_at?: string; requested_from?: string; scope?: 'computer' | 'community'; label?: string };

export function ConnectPage() {
  const [params] = useSearchParams();
  const [code, setCode] = useState(() => params.get('code') ?? '');
  const [device, setDevice] = useState<Device | null>(null);
  const [done, setDone] = useState<'allowed' | 'denied' | null>(null);
  const [error, setError] = useState('');
  const look = async (c: string) => { setError(''); setDevice(null); try { setDevice(await gw('GET', `/device/code/${encodeURIComponent(c)}`)); } catch (e) { setError((e as Error).message); } };
  useEffect(() => { if (code) void look(code); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const answer = async (how: 'approve' | 'deny') => { setError(''); try { await gw('POST', `/device/${how}`, { user_code: device!.user_code }); setDone(how === 'approve' ? 'allowed' : 'denied'); } catch (e) { setError((e as Error).message); } };
  const community = device?.scope === 'community';
  return (
    <main className="hosted-page">
      <h1>{community ? 'Sign in to Community' : 'Connect a computer'}</h1>
      {error && <p className="tone-red" role="alert">{error}</p>}
      {done ? <Done how={done} community={community} /> : device ? <Question device={device} onAnswer={(how) => void answer(how)} /> : (
        <form className="hosted-form" onSubmit={(e) => { e.preventDefault(); void look(code); }}>
          <label>Code shown on the computer<input value={code} onChange={(e) => setCode(e.target.value)} placeholder="WXYZ-2345" autoComplete="off" /></label>
          <button className="btn btn-primary">Continue</button>
        </form>
      )}
    </main>
  );
}

function Done({ how, community }: { how: 'allowed' | 'denied'; community: boolean }) {
  if (how === 'denied') return <p>Turned down. Nothing was connected; the computer shows that the sign-in was turned down.</p>;
  return <p>{community ? 'Signed in. You can close this tab; Sculptured on your computer finishes on its own.' : 'Connected. You can close this tab; the computer finishes on its own.'}</p>;
}

/** What approving lets the device do, who asked and when, then Allow or Don't allow. */
function Question({ device, onAnswer }: { device: Device; onAnswer: (how: 'approve' | 'deny') => void }) {
  const community = device.scope === 'community';
  return (
    <>
      {community ? <CommunityAccess device={device} /> : <ComputerAccess device={device} />}
      {device.requested_at && <p className="muted">Code requested {new Date(device.requested_at).toLocaleString()}{device.requested_from ? ` from ${device.requested_from}` : ''}.</p>}
      <div className="hosted-actions">
        <button className="btn btn-primary" onClick={() => onAnswer('approve')}>Allow</button>{' '}
        <button className="btn" onClick={() => onAnswer('deny')}>Don’t allow</button>
      </div>
    </>
  );
}

function ComputerAccess({ device }: { device: Device }) {
  return (
    <>
      <p>Allow ‘{device.name}’ ({device.platform}) to run agents for you?</p>
      <p className="tone-amber" role="note"><strong>Only approve a code you just saw on your own computer.</strong> If someone sent you this link, don't approve it: their computer would run your teams and see your work.</p>
      <p className="muted">It will run your teams' Claude Code, Codex and Practice agents there. Agents start in the folders that computer allows and, unless it was connected with <code>--sandboxed</code>, can act as you do on that computer. You can disconnect it any time from Computers.</p>
    </>
  );
}

function CommunityAccess({ device }: { device: Device }) {
  return (
    <>
      <p>Allow {device.label ?? `Community access from ‘${device.name}’`}?</p>
      <p className="tone-amber" role="note"><strong>Only approve a code you just saw in Sculptured on your own computer.</strong> If someone sent you this link, don't approve it: they could publish and change your profile as you.</p>
      <p className="muted">Sculptured on that computer can then use Community as you: browse, save, publish and report blocks, and change your profile. It cannot run agents, reach your computers, files or transcripts. You can remove it any time from Computers.</p>
    </>
  );
}
