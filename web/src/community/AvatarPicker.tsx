import { useState } from 'react';
import { COMMUNITY_ERROR_WORDS, errorText } from '../errors';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { Avatar } from './Avatar';
import { AVATAR_MAX_BYTES, AVATAR_TYPES, cropToSquare } from './avatarCrop';

const READ_LIMIT = 5 * 1024 * 1024;

/** Your picture: choose one (cropped to a square here), see it, then use it; or remove it. */
export function AvatarPicker({ current, name, onChanged }: { current: string | null; name: string; onChanged: () => void }) {
  const [picked, setPicked] = useState<{ blob: Blob; url: string } | null>(null);
  const [problem, setProblem] = useState('');
  const choose = async (file: File | undefined) => {
    setProblem('');
    setPicked(null);
    if (!file) return;
    if (!(AVATAR_TYPES as readonly string[]).includes(file.type)) return setProblem(COMMUNITY_ERROR_WORDS.avatar_type);
    if (file.size > READ_LIMIT) return setProblem('That picture is too large to read.');
    try {
      const blob = await cropToSquare(file);
      if (blob.size > AVATAR_MAX_BYTES) return setProblem(COMMUNITY_ERROR_WORDS.avatar_too_large);
      setPicked({ blob, url: URL.createObjectURL(blob) });
    } catch (e) {
      setProblem(errorText(e));
    }
  };
  const act = async (call: () => Promise<unknown>) => {
    try { await call(); setPicked(null); onChanged(); } catch (e) { setProblem(errorText(e)); }
  };
  return (
    <div className="cm-avatar-picker">
      {picked ? <img src={picked.url} alt="Preview of your new picture" width={96} height={96} className="cm-avatar" /> : <Avatar name={name} src={current} size={96} />}
      <label>Choose a picture<input type="file" accept={AVATAR_TYPES.join(',')} onChange={(e) => void choose(e.target.files?.[0])} /></label>
      {picked && <Btn variant="primary" onClick={() => void act(() => communityApi.putAvatar(picked.blob))}>Use this picture</Btn>}
      {current?.startsWith('/api/') && <Btn onClick={() => void act(communityApi.deleteAvatar)}>Remove picture</Btn>}
      {problem && <p className="tone-red" role="alert">{problem}</p>}
    </div>
  );
}
