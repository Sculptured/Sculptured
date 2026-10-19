import { Link } from 'react-router-dom';
import { AvatarPicker } from './AvatarPicker';
import { useCommunity } from './context';
import { ProfileForm } from './ProfileForm';
import { SignInPrompt } from './SignInPrompt';
import './community.css';

/** `/community/me`: edit your profile and picture. */
export function YourProfilePage() {
  const { me, reloadMe } = useCommunity();
  if (!me) return <SignInPrompt why="Sign in to edit your profile." />;
  return (
    <section className="cm-page" aria-label="Your profile">
      <h1>Your profile</h1>
      {me.handle && <p><Link to={`/community/u/${encodeURIComponent(me.handle)}`}>See how others see it</Link></p>}
      <AvatarPicker current={me.avatar_url} name={me.name} onChanged={() => void reloadMe()} />
      <ProfileForm me={me} onSaved={() => void reloadMe()} />
    </section>
  );
}
