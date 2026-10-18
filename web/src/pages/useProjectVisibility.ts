import { useEffect, useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { ProjectVisibilityStatus } from '../generated/ProjectVisibilityStatus';

/** Hidden and trashed projects (kept on the server), and the actions that move them. */
export function useProjectVisibility() {
  const [visibility, setVisibility] = useState<Record<string, ProjectVisibilityStatus>>({});
  const [loadError, setLoadError] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [deleted, setDeleted] = useState<Set<string>>(new Set());
  useEffect(() => {
    let live = true;
    const load = () => api.projectVisibility().then(
      (rows) => live && setVisibility(Object.fromEntries(rows.map((r) => [r.id, r.status]))),
      () => live && setLoadError('Could not load hidden projects. Reload to try again.'),
    );
    void load();
    return () => { live = false; };
  }, []);
  /** True when it worked; the error says why when it did not. */
  const act = async (id: string, fn: () => Promise<void>): Promise<boolean> => {
    setBusy(id); setError('');
    try { await fn(); window.dispatchEvent(new Event('project-visibility-changed')); return true; } catch (e) { setError(errorText(e)); return false; } finally { setBusy(null); }
  };
  const change = (id: string, status: ProjectVisibilityStatus) => act(id, async () => { await api.setProjectVisibility(id, status); setVisibility((old) => ({ ...old, [id]: status })); });
  const deleteForever = (id: string) => act(id, async () => { await api.deleteProjectForever(id); setDeleted((old) => new Set(old).add(id)); });
  const statusOf = (id: string): ProjectVisibilityStatus | 'deleted' => (deleted.has(id) ? 'deleted' : (visibility[id] ?? 'visible'));
  return { statusOf, change, deleteForever, busy, error, loadError };
}
