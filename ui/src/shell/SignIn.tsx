// The sign-in screen (POST /session), and the screen shown when the hub does not answer.
import { useState } from 'preact/hooks';
import { LogIn, RefreshCw, WifiOff } from 'lucide-preact';
import { ApiError } from '../api/client';
import { t } from '../text/i18n';
import { errorText } from '../state/save';
import { boot, publicSystem, signIn } from '../state/session';
import { Button } from '../kit/Button';
import { Banner } from '../kit/Feedback';
import { TextField } from '../kit/Field';
import { Logo } from './Logo';
import s from './SignIn.module.css';

export function SignIn({ overlay }: { overlay?: boolean }) {
  const sys = publicSystem.value;
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const submit = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      await signIn(username.trim(), password);
    } catch (err) {
      setError(err instanceof ApiError && (err.code === 'auth_failed' || err.status === 401) ? t('signIn.wrong') : errorText(err));
      setPassword('');
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class={overlay ? s.overlay : s.screen}>
      <main class={s.card} aria-labelledby="signin-title">
        <div class={s.brand}>
          <Logo />
          <div>
            <h1 id="signin-title" class={s.title}>
              {overlay ? t('signIn.again') : t('signIn.title')}
            </h1>
            <p class={s.sub}>{sys ? `${sys.product} · ${sys.hostname}` : 'SMHUB'}</p>
          </div>
        </div>
        <form class={s.form} onSubmit={submit}>
          {error && (
            <Banner tone="danger" live>
              {error}
            </Banner>
          )}
          <TextField label={t('signIn.username')} value={username} onValue={setUsername} autoComplete="username" required autoFocus autoCapitalize="off" />
          <TextField label={t('signIn.password')} type="password" value={password} onValue={setPassword} autoComplete="current-password" required />
          <Button type="submit" variant="primary" size="lg" block icon={LogIn} busy={busy}>
            {t('signIn.submit')}
          </Button>
        </form>
        <p class={s.help}>{t('signIn.help')}</p>
      </main>
    </div>
  );
}

export function Offline() {
  const [busy, setBusy] = useState(false);
  return (
    <div class={s.screen}>
      <main class={s.card} aria-labelledby="offline-title">
        <div class={s.offlineIcon} aria-hidden="true">
          <WifiOff size={28} />
        </div>
        <h1 id="offline-title" class={s.title}>
          {t('offline.title')}
        </h1>
        <p class={s.text} role="status">
          {t('offline.text')}
        </p>
        <Button
          variant="primary"
          icon={RefreshCw}
          busy={busy}
          onClick={async () => {
            setBusy(true);
            await boot();
            setBusy(false);
          }}
        >
          {t('offline.retry')}
        </Button>
      </main>
    </div>
  );
}

/** Shown only if the first answers take a moment (it fades in late). */
export function Splash() {
  return (
    <div class={s.splash} role="status">
      <Logo />
      <span class="visually-hidden">{t('common.loading')}</span>
    </div>
  );
}
