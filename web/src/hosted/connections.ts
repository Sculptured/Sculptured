import { useCallback, useEffect, useState } from 'react';
import { gw } from './session';

export type ConnectionProvider = 'anthropic-key' | 'openrouter';
export type ConnectionCheck = { ok: boolean; at: string; detail: string };
/** A model connection kept by the gateway. Its key never comes back: only its last four characters. */
export type ModelConnection = {
  id: string; name: string; provider: ConnectionProvider; model: string | null;
  key_last4: string; checked: ConnectionCheck | null; created_at: string;
};
export type NewConnection = { name: string; provider: ConnectionProvider; key: string; model?: string };

export const PROVIDER_LABEL: Record<ConnectionProvider, string> = { 'anthropic-key': 'Claude (API key)', openrouter: 'Open models via OpenRouter' };
/** An open model OpenCode agents run well, by its OpenRouter name (OpenCode calls it `openrouter/…`). */
export const OPENROUTER_MODEL = 'moonshotai/kimi-k2.7-code';

/** What runs on a connection: Claude Code on an Anthropic key, OpenCode on an OpenRouter key. */
export const connectionLabel = (c: Pick<ModelConnection, 'provider' | 'name'>) =>
  `${c.provider === 'openrouter' ? 'OpenCode via OpenRouter' : PROVIDER_LABEL[c.provider]} · ${c.name}`;
export const maskedKey = (last4: string) => `••••${last4}`;

export async function listConnections(): Promise<ModelConnection[]> {
  const r = await gw<{ connections?: ModelConnection[] }>('GET', '/connections');
  return Array.isArray(r?.connections) ? r.connections : [];
}
export const addConnection = (c: NewConnection) => gw<ModelConnection>('POST', '/connections', c);
/** Checks the key without spending tokens; answers with the connection, its `checked` filled in. */
export const checkConnection = (id: string) => gw<ModelConnection>('POST', `/connections/${encodeURIComponent(id)}/check`);
export const deleteConnection = (id: string) => gw<unknown>('DELETE', `/connections/${encodeURIComponent(id)}`);

/** Your model connections (hosted only; none elsewhere or when the gateway has none). */
export function useConnections(enabled: boolean) {
  const [list, setList] = useState<ModelConnection[]>([]);
  const [loaded, setLoaded] = useState(false);
  const reload = useCallback(() => {
    if (!enabled) return Promise.resolve();
    return listConnections().then(setList, () => setList([])).finally(() => setLoaded(true));
  }, [enabled]);
  useEffect(() => { void reload(); }, [reload]);
  return { list, loaded, reload };
}
