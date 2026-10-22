import { setCsrf } from '../api';

export type HostedUser = { id: string; name: string; email: string; role: 'member' | 'admin' };
export type HostedSession = { kind: 'local' } | { kind: 'signed-out' } | { kind: 'signed-in'; user: HostedUser; csrf: string };

let csrf: string | null = null;

/** Local Sculptured has no `/gw/api/me` (404); the hosted gateway answers 401 or the user. */
export async function loadSession(): Promise<HostedSession> {
  const res = await fetch('/gw/api/me', { credentials: 'same-origin' });
  if (res.status === 404) return { kind: 'local' };
  if (res.status === 401) { csrf = null; setCsrf(null); return { kind: 'signed-out' }; }
  if (!res.ok) throw new GwError(res.status, 'unavailable', 'Sculptured is not answering right now.');
  // Local Sculptured answers this path with the app itself (not JSON): that is local mode.
  const body = await res.json().catch(() => null);
  if (!body?.user || !body.csrf) return { kind: 'local' };
  csrf = body.csrf; setCsrf(body.csrf);
  return { kind: 'signed-in', user: body.user, csrf: body.csrf };
}

export class GwError extends Error { constructor(public status: number, public code: string, message: string) { super(message); } }

export async function gw<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['content-type'] = 'application/json';
  if (method !== 'GET' && csrf) headers['x-csrf-token'] = csrf;
  const res = await fetch(`/gw/api${path}`, { method, headers, credentials: 'same-origin', body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  const json = text ? JSON.parse(text) : {};
  if (!res.ok) throw new GwError(res.status, json.error?.code ?? 'http_error', json.error?.message ?? `HTTP ${res.status}`);
  return json as T;
}

export const providerHref = (purpose: { join: string } | 'login', provider: 'google' | 'github') =>
  purpose === 'login' ? `/gw/api/login/oidc/${provider}` : `/gw/api/join/${purpose.join}/oidc/${provider}`;

export const RECORDED = 'We record what you do in Sculptured to improve the beta: pages and features you use and for how long, the shape of the teams you set up (how many agents, which kinds and models), runs, token totals and errors. We never record your prompts, messages, jobs, agent output, file names or file contents.';
