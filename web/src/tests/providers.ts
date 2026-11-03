// Model providers of API-mode agents (arena `providers/`): names, states, choices and the checked list.
import { useEffect, useSyncExternalStore } from 'react';
import { arena } from './api';
import { DEFAULT_OPENROUTER_MODEL, DEFAULT_PROVIDER } from './types';
import type { AgentSpec, Change, ProviderKind, ProviderStatus } from './types';

/** The cost source of a local server's calls (arena `providers/mod.rs` `LOCAL_COST_SOURCE`). */
export const LOCAL_COST_SOURCE = 'local (no API charge)';

/** The provider id `p` stands for: itself trimmed, else the default (`openrouter`). */
export function providerId(p: string | null | undefined): string {
  const t = p?.trim();
  return t ? t : DEFAULT_PROVIDER;
}

/** The provider an agent's model calls go to. */
export const providerOf = (a: { provider?: string | null }) => providerId(a.provider);

export function kindOf(id: string): ProviderKind {
  return id === 'openrouter' || id === 'openai' || id === 'anthropic' ? id : 'local';
}

/** A short name: OpenRouter, OpenAI, Anthropic, or `<name> (local)`. */
export function providerName(id: string): string {
  const names: Record<ProviderKind, string> = { openrouter: 'OpenRouter', openai: 'OpenAI', anthropic: 'Anthropic', local: '' };
  return names[kindOf(id)] || `${id.replace(/^local:/, '')} (local)`;
}

/** Only OpenRouter has a default model; the others need one named. */
export const hasDefaultModel = (id: string) => kindOf(id) === 'openrouter';

/** OpenAI and Anthropic report no price, and Arena keeps no price table. */
export const costIsUnknown = (id: string) => kindOf(id) === 'openai' || kindOf(id) === 'anthropic';

/** The provider shown next to an agent's model: an API-mode agent's, else none. */
export const agentProvider = (a: Pick<AgentSpec, 'kind' | 'provider'>): string | null =>
  a.kind === 'open' ? providerName(providerOf(a)) : null;

export type ProviderState = 'ready' | 'unavailable' | 'not checked';

/** Ready, unavailable, or (a local server the health check didn't ask) not checked. */
export function providerState(s: ProviderStatus): ProviderState {
  if (s.available) return 'ready';
  return s.kind === 'local' && !s.checked ? 'not checked' : 'unavailable';
}

const GLYPH: Record<ProviderState, string> = { ready: '✓', unavailable: '✕', 'not checked': '?' };
export const stateGlyph = (st: ProviderState) => GLYPH[st];

/** The reason shown next to the state (none for "not checked": the word says it). */
export const stateReason = (s: ProviderStatus): string | null =>
  providerState(s) === 'not checked' ? null : s.why ?? null;

/** Some provider can take API-mode calls (a local server not checked yet counts); `fallback` when none are listed. */
export const someProviderOk = (list: ProviderStatus[], fallback: boolean) =>
  list.length ? list.some((s) => providerState(s) !== 'unavailable') : fallback;

/** An option of a provider select: unavailable ones are disabled and say why. */
export interface ProviderOption {
  id: string;
  text: string;
  disabled: boolean;
}

/**
 * The providers to choose from. `current` (the agent's provider) is always listed and never
 * disabled, so a team keeps showing what it names; one Arena doesn't know is "not configured".
 */
export function providerOptions(list: ProviderStatus[], current: string): ProviderOption[] {
  const out: ProviderOption[] = list.map((s) => {
    const st = providerState(s);
    const why = stateReason(s);
    const text = st === 'ready' ? s.label : `${s.label} (${st}${why ? `: ${why}` : ''})`;
    return { id: s.id, text, disabled: st === 'unavailable' && s.id !== current };
  });
  if (!list.some((s) => s.id === current)) {
    out.push({ id: current, text: list.length ? `${current} (not configured)` : providerName(current), disabled: false });
  }
  return out;
}

/** Why an API-mode agent on `provider` with `model` can't run, if it can be told here. */
export function modelProblem(provider: string, model: string | null | undefined, list: ProviderStatus[]): string | null {
  const s = list.find((x) => x.id === provider);
  if (!s && list.length) return `${provider} is not configured on this Arena`;
  if (s && providerState(s) === 'unavailable') return `${s.label} is unavailable${s.why ? `: ${s.why}` : ''}`;
  if (!model?.trim() && !hasDefaultModel(provider)) return `Name a model for ${s?.label ?? providerName(provider)}`;
  return null;
}

/** Why a team (or a fork's agents) can't run: the first API-mode agent's problem, with its name. */
export function agentsProblem(agents: AgentSpec[], list: ProviderStatus[]): string | null {
  for (const a of agents) {
    const p = a.kind === 'open' ? modelProblem(providerOf(a), a.model, list) : null;
    if (p) return `${a.name}: ${p}`;
  }
  return null;
}

/** The agent a fork would run: `c` applied as arenad's `fork::apply_change` does. */
export function applied(a: AgentSpec, c: Change): AgentSpec {
  const kindChanged = c.kind != null && c.kind !== a.kind;
  const providerChanged = c.provider != null && providerId(c.provider) !== providerOf(a);
  // The old model belongs to the old mode or provider: the new one's default applies.
  let model = kindChanged || providerChanged ? null : a.model;
  if (c.model != null) model = c.model.trim() || null;
  return { ...a, prompt: c.prompt ?? a.prompt, kind: c.kind ?? a.kind, provider: c.provider ?? a.provider, model };
}

/** Why a fork can't run a changed API-mode agent (an unavailable provider, or no model named). */
export const forkProblem = (agents: AgentSpec[], changes: Record<string, Change>, list: ProviderStatus[]) =>
  agentsProblem(agents.flatMap((a) => (changes[a.id] ? [applied(a, changes[a.id])] : [])), list);

/** The model box's hint: the default, or that a model must be named. */
export const modelPlaceholder = (provider: string) =>
  hasDefaultModel(provider) ? `${DEFAULT_OPENROUTER_MODEL} (default)` : 'model id (required)';

// ---------------------------------------------------------------- the checked list, shared

/** The last `/api/providers` answer, shared by every pane on the page. */
type Snapshot = { list: ProviderStatus[] | null; loading: boolean; error: unknown };
let snapshot: Snapshot = { list: null, loading: false, error: null };
let pending: Promise<ProviderStatus[]> | null = null;
const listeners = new Set<() => void>();
const set = (s: Partial<Snapshot>) => { snapshot = { ...snapshot, ...s }; listeners.forEach((f) => f()); };

/** Asks `/api/providers` (once at a time) and shares the answer. */
export function checkProviders(): Promise<ProviderStatus[]> {
  if (pending) return pending;
  pending = arena.providers()
    .then((list) => { set({ list, error: null }); return list; })
    .catch((error: unknown) => { set({ error }); throw error; })
    .finally(() => { pending = null; set({ loading: false }); });
  set({ loading: true });
  return pending;
}

/** For tests: forget the shared answer. */
export function resetProviders(list: ProviderStatus[] | null = null): void {
  pending = null;
  snapshot = { list, loading: false, error: null };
}

function subscribe(f: () => void) {
  listeners.add(f);
  return () => { listeners.delete(f); };
}

/**
 * The providers to show and pick from: the checked list once `/api/providers` answers (asked on
 * mount unless known), until then `health` (the health check's, locals not checked).
 */
export function useProviderList(health: ProviderStatus[] | undefined): Snapshot & { list: ProviderStatus[]; check(): void } {
  const s = useSyncExternalStore(subscribe, () => snapshot, () => snapshot);
  useEffect(() => {
    if (!snapshot.list) checkProviders().catch(() => {});
  }, []);
  return { ...s, list: s.list ?? health ?? [], check: () => { checkProviders().catch(() => {}); } };
}
