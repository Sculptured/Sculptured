import { initials } from '../shell/initials';
import { safeAvatarUrl } from './api';

/** A steady colour for a name, so the same member keeps the same initials colour. */
function hue(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.codePointAt(0)!) % 360;
  return h;
}

/** A member's picture: theirs when it is one of Community's own, else their initials. */
export function Avatar({ name, src, size = 32 }: { name: string; src?: string | null; size?: number }) {
  const safe = safeAvatarUrl(src);
  if (safe) return <img className="cm-avatar" src={safe} alt="" width={size} height={size} />;
  return (
    <span className="cm-avatar cm-initials" aria-hidden="true" style={{ width: size, height: size, background: `hsl(${hue(name)} 45% 42%)`, fontSize: size * 0.4 }}>
      {initials(name)}
    </span>
  );
}
