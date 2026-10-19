import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { COMMUNITY_ERROR_WORDS } from '../errors';
import { AvatarPicker } from './AvatarPicker';
import { cropToSquare } from './avatarCrop';
import { fakeFetch } from './testing';

vi.mock('./avatarCrop', async (orig) => ({ ...(await orig<typeof import('./avatarCrop')>()), cropToSquare: vi.fn() }));
afterEach(() => vi.restoreAllMocks());
globalThis.URL.createObjectURL ??= () => 'blob:preview';

const pick = (bytes: number, type = 'image/png') => new File([new Uint8Array(bytes)], 'me.png', { type });

it('previews the cropped picture and uploads it on confirm', async () => {
  vi.mocked(cropToSquare).mockResolvedValue(new Blob([new Uint8Array(1000)], { type: 'image/webp' }));
  const changed = vi.fn();
  const { calls, inits } = fakeFetch({ 'PUT /api/community/me/avatar': () => [200, { avatar_url: '/api/community/users/ana/avatar?v=1' }] });
  render(<AvatarPicker current={null} name="Ana" onChanged={changed} />);
  const user = userEvent.setup();
  await user.upload(screen.getByLabelText('Choose a picture'), pick(10_000));
  expect(await screen.findByRole('img', { name: 'Preview of your new picture' })).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Use this picture' }));
  await vi.waitFor(() => expect(changed).toHaveBeenCalled());
  expect(calls).toEqual(['PUT /api/community/me/avatar']);
  expect((inits[0]!.headers as Record<string, string>)['content-type']).toBe('image/webp');
});

it('a picture still over 256 KB after cropping is refused before upload', async () => {
  vi.mocked(cropToSquare).mockResolvedValue(new Blob([new Uint8Array(262145)], { type: 'image/webp' }));
  const { calls } = fakeFetch({});
  render(<AvatarPicker current={null} name="Ana" onChanged={() => {}} />);
  await userEvent.setup().upload(screen.getByLabelText('Choose a picture'), pick(1_000_000));
  expect(await screen.findByText(COMMUNITY_ERROR_WORDS.avatar_too_large)).toBeVisible();
  expect(calls).toEqual([]);
});

it('a wrong type or a huge file is refused before reading it', async () => {
  render(<AvatarPicker current={null} name="Ana" onChanged={() => {}} />);
  const user = userEvent.setup({ applyAccept: false });
  await user.upload(screen.getByLabelText('Choose a picture'), pick(10, 'image/gif'));
  expect(await screen.findByText(COMMUNITY_ERROR_WORDS.avatar_type)).toBeVisible();
  await user.upload(screen.getByLabelText('Choose a picture'), pick(6_000_000));
  expect(await screen.findByText('That picture is too large to read.')).toBeVisible();
  expect(cropToSquare).not.toHaveBeenCalled();
});

it('server picture errors are shown in words, and Remove picture removes it', async () => {
  vi.mocked(cropToSquare).mockResolvedValue(new Blob([new Uint8Array(10)], { type: 'image/webp' }));
  const { calls } = fakeFetch({ 'PUT /api/community/me/avatar': () => [415, { error: { code: 'avatar_type', message: 'x' } }], 'DELETE /api/community/me/avatar': () => [200, { avatar_url: 'data:image/svg+xml;base64,x' }] });
  render(<AvatarPicker current="/api/community/users/ana/avatar?v=1" name="Ana" onChanged={() => {}} />);
  const user = userEvent.setup();
  await user.upload(screen.getByLabelText('Choose a picture'), pick(100));
  await user.click(await screen.findByRole('button', { name: 'Use this picture' }));
  expect(await screen.findByText(COMMUNITY_ERROR_WORDS.avatar_type)).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Remove picture' }));
  await vi.waitFor(() => expect(calls).toContain('DELETE /api/community/me/avatar'));
});
