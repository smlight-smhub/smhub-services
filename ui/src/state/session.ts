// Sign-in state. A 401 from any request shows the sign-in screen over the
// page; signing in again continues where the user was.
import { batch, signal } from '@preact/signals';
import { api, ApiError, onUnauthorized } from '../api/client';
import type { Preferences, Session, System } from '../api/types';
import { pickLanguage, setLanguage, lang } from '../text/i18n';
import { pageView } from './get';
import { prefs, restoreLocalPrefs } from './prefs';
import { onConnectFailed, setLive, startEvents, stopEvents } from './store';
import { reloadIfOutdated } from './version';

/** loading: first answers pending; in: signed in (or open hub); out: sign-in needed; offline: the hub does not answer. */
export type AuthState = 'loading' | 'in' | 'out' | 'offline';

export const auth = signal<AuthState>('loading');
export const session = signal<Session | null>(null);
/** The example in an empty user-name field when sign-in is turned on: a name of the owner's own,
 *  not the factory account's (the hub locks that one while sign-in is on). */
export const USERNAME_EXAMPLE = 'user@localhost.com';
/** The public part of `system`, for the sign-in screen. */
export const publicSystem = signal<System | null>(null);

onUnauthorized(() => {
  if (auth.peek() === 'in') signedOut();
});

let checking = false;
onConnectFailed(() => {
  // The socket would not open: if the session ended, the GET answers 401
  // and the handler above shows the sign-in screen.
  if (checking || auth.peek() !== 'in') return;
  checking = true;
  api
    .get('/session')
    .catch(() => undefined)
    .finally(() => (checking = false));
});

function signedOut() {
  stopEvents();
  batch(() => {
    session.value = null;
    auth.value = 'out';
  });
  void loadPublicSystem();
}

async function loadPublicSystem() {
  try {
    const s = await api.get('/system', { quiet: true });
    publicSystem.value = s;
    setLive('system', s);
  } catch {
    /* the sign-in screen works without it */
  }
}

async function enter(s: Session, p: Preferences | null, sys: System | null) {
  if (p && pickLanguage(p.language) !== lang.peek()) await setLanguage(p.language);
  batch(() => {
    if (sys) {
      publicSystem.value = sys;
      setLive('system', sys);
    }
    session.value = s;
    if (p) prefs.value = p;
    auth.value = 'in';
  });
  startEvents();
}

let retry = 0;
/**
 * Starts the UI: the session, the preferences and `system` (public: what the
 * hub has, and whether it needs its first-run setup), all in parallel, so the
 * first paint already knows them.
 */
export async function boot(): Promise<void> {
  const localReady = restoreLocalPrefs();
  const systemReq = api.get('/system', { quiet: true }).catch(() => null);
  try {
    const [s, p, sys] = await Promise.all([api.get('/session', { quiet: true }), api.get('/preferences', { quiet: true }).catch(() => null), systemReq, localReady]);
    retry = 0;
    // A cached page older (or newer) than the installed UI reloads before it shows anything.
    if (reloadIfOutdated(sys)) return;
    await enter(s, p, sys);
  } catch (e) {
    await localReady.catch(() => undefined);
    const sys = await systemReq;
    if (reloadIfOutdated(sys)) return;
    if (e instanceof ApiError && e.status === 401) {
      batch(() => {
        if (sys) {
          publicSystem.value = sys;
          setLive('system', sys);
        }
        auth.value = 'out';
      });
    } else {
      auth.value = 'offline';
      const delay = Math.min(15_000, 1000 * 2 ** retry++);
      setTimeout(() => void boot(), delay);
    }
  }
}

export async function signIn(username: string, password: string): Promise<void> {
  const s = await api.post('/session', { body: { username, password }, quiet: true });
  const p = await api.get('/preferences', { quiet: true }).catch(() => null);
  pageView.value++; // pages read their data again
  await enter(s, p, publicSystem.peek());
}

/** Sign-in became required (POST /security/enable answers the new session). */
export function sessionChanged(s: Session) {
  session.value = s;
}

export async function signOut(): Promise<void> {
  try {
    await api.del('/session', { quiet: true });
  } catch {
    /* signed out anyway */
  }
  location.assign('/');
}
