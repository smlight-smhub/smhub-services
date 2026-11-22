// The root: decides between the splash, sign-in, first-run setup and the
// shell. A sign-in needed mid-use is shown over the page, so the user
// continues where they were.
import { useRef } from 'preact/hooks';
import { auth } from '../state/session';
import { needsSetup } from './firstRun';
import { Setup } from './routes';
import { Shell } from './Shell';
import { Offline, SignIn, Splash } from './SignIn';

export function App() {
  const a = auth.value;
  const shown = useRef(false);
  if (a === 'in') shown.current = true;
  if (a === 'loading') return <Splash />;
  if (!shown.current) {
    if (a === 'offline') return <Offline />;
    if (a === 'out') return <SignIn />;
  }
  if (needsSetup.value) return <Setup />;
  return (
    <>
      <Shell inert={a !== 'in'} />
      {a === 'out' && <SignIn overlay />}
    </>
  );
}
