import { act, renderHook } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { communityFor, hostedLink, resetCommunityLink, useCommunityLink } from './link';
import { fakeFetch } from './testing';

afterEach(() => { vi.restoreAllMocks(); resetCommunityLink(); });

it('the hosted app signs in to Community with the Sculptured account', () => {
  expect(communityFor({ kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x', role: 'member' }, csrf: 't' })).toEqual({ available: true, signedIn: true });
  expect(communityFor({ kind: 'signed-out' })).toEqual({ available: true, signedIn: false });
});

it('hosted mode derives the link from the session and never fetches /api/community-link', () => {
  const { calls } = fakeFetch({});
  const s: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x', role: 'member' }, csrf: 't' };
  expect(hostedLink(s)).toEqual({ enabled: true, origin: null, signed_in: true, account: { name: 'Ana', handle: null } });
  expect(hostedLink({ kind: 'local' })).toBeNull();
  const wrapper = ({ children }: { children: ReactNode }) => createElement(HostedProvider, { value: s }, children);
  const { result } = renderHook(() => useCommunityLink(), { wrapper });
  expect(result.current.link?.signed_in).toBe(true);
  expect(calls).toEqual([]);
});

it('local mode fetches the link once for every subscriber and refresh updates them', async () => {
  let signedIn = false;
  const { calls } = fakeFetch({ 'GET /api/community-link': () => [200, { enabled: true, origin: 'http://stub', signed_in: signedIn, account: null }] });
  const a = renderHook(() => useCommunityLink());
  const b = renderHook(() => useCommunityLink());
  await vi.waitFor(() => expect(a.result.current.link?.enabled).toBe(true));
  expect(calls).toEqual(['GET /api/community-link']);
  signedIn = true;
  await act(() => a.result.current.refresh());
  expect(b.result.current.link?.signed_in).toBe(true);
});
