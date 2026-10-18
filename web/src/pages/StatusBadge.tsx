import { Icon } from '../shell/icons';
import type { ProjectStatus } from './status';

/** A status as a drawn shape plus words, tinted by its tone (never colour alone). */
export function StatusBadge({ s, className }: { s: ProjectStatus; className?: string }) {
  return (
    <span className={`pstatus ps-${s.tone}${className ? ` ${className}` : ''}`}>
      <Icon name={s.icon} size={13} className={s.tone === 'working' ? 'ps-icon spin-slow' : 'ps-icon'} />
      <span className="ps-text">{s.text}</span>
    </span>
  );
}
