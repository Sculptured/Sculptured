import { Link } from 'react-router-dom';
import type { Author } from '../generated/community/Author';
import { Avatar } from './Avatar';

/** Who published a block, linking to their profile. */
export function AuthorChip({ author }: { author: Author }) {
  return (
    <Link className="cm-author" to={`/community/u/${encodeURIComponent(author.handle)}`}>
      <Avatar name={author.name} src={author.avatar_url} size={24} />
      <span>{author.name}</span>
    </Link>
  );
}
