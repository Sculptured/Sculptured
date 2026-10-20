import { Link } from 'react-router-dom';
import { AvatarPicker } from './AvatarPicker';
import { useCommunity } from './context';
import { MeGate } from './MeGate';
import { ProfileForm } from './ProfileForm';
import './community.css';

/** `/community/me`: edit your profile and picture. */
export function YourProfilePage() {
  const { reloadMe } = useCommunity();
  return (
    <MeGate why="Sign in to edit your profile.">
      {(me) => (
        <section className="cm-page" aria-label="Your profile">
          <h1>Your profile</h1>
          {me.handle && <p><Link to={`/community/u/${encodeURIComponent(me.handle)}`}>See how others see it</Link></p>}
          {me.handle
            ? <AvatarPicker current={me.avatar_url} name={me.name} onChanged={() => void reloadMe()} />
            : <p className="muted">Choose a handle first (below); then you can add a picture.</p>}
          <ProfileForm me={me} onSaved={() => void reloadMe()} />
        </section>
      )}
    </MeGate>
  );
}
