import { useLook } from '../appearance';
import { Icon } from './icons';

/**
 * The brand mark. Coastal shows the supplied emblem artwork unchanged, framed at compact sizes.
 * Standard is flat: the plain mark on an indigo tile (--brand-mark-bg), never a white emblem plate.
 */
export function BrandMark() {
  const look = useLook();
  if (look === 'standard') return <span className="brand-mark plain-mark" aria-hidden="true"><Icon name="logo" size={15} strokeWidth={2.2} /></span>;
  return <span className="coastal-mark" aria-hidden="true"><img src="/brand/sculptured-coastal-emblem-256.png" alt="" width="256" height="256" /></span>;
}
