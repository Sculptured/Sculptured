import { useState } from 'react';
import { gw } from './session';
import type { Cluster } from './Clusters';

/** A certificate cluster: whom the certificate names and until when, and a place for a new one
 * (offered at once when it has ended or ends within 2 hours). */
export function CertificateLine({ c, reload }: { c: Cluster; reload: () => void }) {
  const cert = c.certificate!;
  const until = cert.valid_before ? new Date(cert.valid_before).toLocaleString() : 'no end';
  const warning = cert.warning === 'expired' ? 'Certificate expired — paste a new one' : cert.warning === 'soon' ? `Certificate expires ${until} — paste a new one` : null;
  return (
    <div className="cluster-key">
      <p className="muted">Certificate for {cert.principals.join(', ') || 'any user'}{cert.key_id ? ` (${cert.key_id})` : ''}, until {until}.</p>
      {warning && <p className="tone-amber" role="status">{warning}</p>}
      <details open={!!warning}>
        <summary>Paste a new certificate</summary>
        <NewCertificate id={c.id} reload={reload} />
      </details>
    </div>
  );
}

function NewCertificate({ id, reload }: { id: string; reload: () => void }) {
  const [cert, setCert] = useState('');
  const [key, setKey] = useState('');
  const [error, setError] = useState('');
  const save = async () => {
    setError('');
    try {
      await gw('POST', `/clusters/${id}/certificate`, key.trim() ? { certificate: cert, private_key: key } : { certificate: cert });
      setCert(''); setKey(''); reload();
    } catch (e) { setError((e as Error).message); }
  };
  return (
    <div className="hosted-form">
      <label>New certificate (-cert.pub)<textarea rows={3} value={cert} onChange={(e) => setCert(e.target.value)} spellCheck={false} /></label>
      <label>Its private key (if it changed)<textarea rows={4} value={key} onChange={(e) => setKey(e.target.value)} spellCheck={false} autoComplete="off" /></label>
      <button type="button" className="btn" onClick={() => void save()} disabled={!cert.trim()}>Save certificate</button>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}
