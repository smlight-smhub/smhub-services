// First-run setup (D38): language and region, units, the hub's name, its
// time zone, an optional password, and how much detail to show. Every step
// saves as it goes; finishing (or skipping) saves the dashboard layout, which
// marks the hub as set up.
import { useEffect, useRef, useState } from 'preact/hooks';
import { ArrowLeft, ArrowRight, Check, History, Lock, LockOpen } from 'lucide-preact';
import { api } from '../api/client';
import { t } from '../text/i18n';
import { browserTimeZone, dateTime } from '../text/format';
import { confirm } from '../state/confirm';
import { useGet } from '../state/get';
import { startOperation } from '../state/operations';
import { effectivePrefs, savePrefs } from '../state/prefs';
import { save, useFieldErrors } from '../state/save';
import { USERNAME_EXAMPLE, sessionChanged } from '../state/session';
import { live, setLive, useLive } from '../state/store';
import { Button } from '../kit/Button';
import { Banner } from '../kit/Feedback';
import { Choice, TextField } from '../kit/Field';
import { SearchSelect } from '../kit/SearchSelect';
import { cx } from '../kit/cx';
import { LanguageChoice, Preview, RegionSelect, UnitChoices } from '../pages/settings/Regional';
import { HOSTNAME } from '../pages/settings/General';
import { setupDone } from './firstRun';
import { ConfirmHost, Toasts } from './Overlays';
import { Logo } from './Logo';
import s from './Setup.module.css';

const STEPS = ['welcome', 'language', 'units', 'name', 'time', 'password', 'done'] as const;
type Step = (typeof STEPS)[number];

async function finish(level?: 'simple' | 'advanced') {
  if (level && level !== effectivePrefs.peek().level) void savePrefs({ level });
  await setupDone();
}

export default function Setup() {
  useLive('system');
  useLive('operations');
  const sys = live('system').value;
  const factory = useGet('/backups/factory');
  const zones = useGet('/settings/timezones');
  const [step, setStep] = useState<Step>('welcome');
  const [name, setName] = useState('');
  const [zone, setZone] = useState('');
  const [user, setUser] = useState('');
  const [pw, setPw] = useState('');
  const [again, setAgain] = useState('');
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const [level, setLevel] = useState<'simple' | 'advanced'>(effectivePrefs.value.level);
  const errors = useFieldErrors();
  const heading = useRef<HTMLHeadingElement>(null);
  const i = STEPS.indexOf(step);

  useEffect(() => {
    if (sys && !name) setName(sys.hostname);
    if (sys && !zone) {
      // A new hub is often on UTC; the browser's zone is the better guess.
      const guess = browserTimeZone();
      setZone(sys.timezone && !/^(Etc\/)?UTC$/.test(sys.timezone) ? sys.timezone : guess || sys.timezone);
    }
  }, [sys]);
  useEffect(() => {
    document.title = t('setup.title');
    if (i > 0) heading.current?.focus();
    setTried(false);
  }, [step]);

  const next = () => setStep(STEPS[Math.min(STEPS.length - 1, i + 1)]!);
  const back = () => setStep(STEPS[Math.max(0, i - 1)]!);
  const nameError = name && !HOSTNAME.test(name) ? t('general.nameInvalid') : undefined;
  const pwErrors: Record<string, string> = {};
  if (pw || tried) {
    if (pw.length < 8) pwErrors.password = t('accounts.tooShort');
    if (again !== pw) pwErrors.again = t('accounts.mismatch');
  }

  const general = async (body: { hostname?: string; timezone?: string }) => {
    setBusy(true);
    const r = await save({ send: () => api.patch('/settings/general', { body }), errors });
    setBusy(false);
    if (r && sys) setLive('system', { ...sys, hostname: r.hostname ?? sys.hostname, timezone: r.timezone ?? sys.timezone });
    return !!r;
  };
  const onNext = async () => {
    if (step === 'name') {
      if (!name || nameError) return;
      if (name !== sys?.hostname && !(await general({ hostname: name }))) return;
    } else if (step === 'time') {
      if (zone && zone !== sys?.timezone && !(await general({ timezone: zone }))) return;
    } else if (step === 'password' && pw) {
      setTried(true);
      if (Object.keys(pwErrors).length || !user.trim()) return;
      setBusy(true);
      const sess = await save({ send: () => api.post('/security/enable', { body: { username: user.trim(), password: pw } }), errors, success: t('accounts.enabled') });
      setBusy(false);
      if (!sess) return;
      sessionChanged(sess);
    }
    next();
  };
  const restoreFactory = async () => {
    if (!(await confirm({ title: t('backups.factoryRestoreTitle'), body: t('backups.factoryRestoreBody'), confirm: t('backups.restoreNow'), tone: 'danger' }))) return;
    const op = await startOperation(() => api.post('/backups/factory/restore'));
    if (op) await finish();
  };

  const title: Record<Step, string> = {
    welcome: t('setup.welcome', { product: sys?.product ?? 'SMHUB' }),
    language: t('setup.languageTitle'),
    units: t('setup.unitsTitle'),
    name: t('setup.nameTitle'),
    time: t('setup.timeTitle'),
    password: t('setup.passwordTitle'),
    done: t('setup.doneTitle'),
  };

  return (
    <div class={s.screen}>
      <main class={s.card} aria-labelledby="setup-title">
        <header class={s.head}>
          <Logo />
          {i > 0 && i < STEPS.length - 1 && (
            <p class={s.count} aria-live="polite">
              {t('setup.step', { n: String(i), total: String(STEPS.length - 2) })}
            </p>
          )}
          <button type="button" class={s.skip} onClick={() => void finish()}>
            {t('setup.skip')}
          </button>
        </header>
        <ol class={s.dots} aria-hidden="true">
          {STEPS.slice(1, -1).map((st, j) => (
            <li key={st} class={cx(s.dot, j + 1 <= i && s.dotOn)} />
          ))}
        </ol>
        <form
          class={s.body}
          key={step}
          onSubmit={(e) => {
            e.preventDefault();
            void onNext();
          }}
          noValidate
        >
          <h1 id="setup-title" ref={heading} tabIndex={-1} class={s.title}>
            {title[step]}
          </h1>
          {step === 'welcome' && (
            <>
              <p class={s.lead}>{t('setup.welcomeText')}</p>
              {factory.data?.available && (
                <Banner
                  tone="info"
                  icon={History}
                  title={t('setup.factoryTitle')}
                  action={
                    <Button size="sm" onClick={() => void restoreFactory()}>
                      {t('backups.restoreNow')}
                    </Button>
                  }
                >
                  {t('setup.factoryText', { date: factory.data.created ? dateTime(factory.data.created) : '' })}
                </Banner>
              )}
            </>
          )}
          {step === 'language' && (
            <>
              <LanguageChoice />
              <RegionSelect />
            </>
          )}
          {step === 'units' && (
            <>
              <UnitChoices />
              <div class={s.preview}>
                <Preview />
              </div>
            </>
          )}
          {step === 'name' && (
            <>
              <p class={s.lead}>{t('setup.nameText')}</p>
              <TextField
                label={t('general.hostname')}
                value={name}
                onValue={setName}
                autoFocus
                maxLength={63}
                autoCapitalize="off"
                spellcheck={false}
                hint={t('general.nameHint', { address: `${(name || 'smhub').toLowerCase()}.local` })}
                error={nameError ?? errors.value.hostname}
              />
            </>
          )}
          {step === 'time' && (
            <>
              <p class={s.lead}>{t('setup.timeText')}</p>
              <SearchSelect label={t('general.timeZone')} value={zone} onValue={setZone} options={(zones.data ?? (zone ? [zone] : [])).map((z) => ({ value: z, label: z.replace(/_/g, ' ') }))} error={errors.value.timezone} />
            </>
          )}
          {step === 'password' && (
            <>
              <p class={s.lead}>{t('setup.passwordText')}</p>
              <TextField
                label={t('accounts.username')}
                value={user}
                onValue={setUser}
                placeholder={USERNAME_EXAMPLE}
                autoComplete="username"
                autoCapitalize="off"
                error={errors.value.username ?? (tried && pw && !user.trim() ? t('form.required') : undefined)}
              />
              <TextField label={t('accounts.password')} type="password" value={pw} onValue={setPw} autoComplete="new-password" hint={t('accounts.passwordHint')} error={pwErrors.password ?? errors.value.password} />
              <TextField label={t('accounts.again')} type="password" value={again} onValue={setAgain} autoComplete="new-password" error={pwErrors.again} />
              <p class={cx(s.lead, s.small)}>
                {pw ? <Lock size={16} aria-hidden="true" /> : <LockOpen size={16} aria-hidden="true" />}
                {pw ? t('setup.passwordWill') : t('setup.passwordSkip')}
              </p>
            </>
          )}
          {step === 'done' && (
            <>
              <p class={s.lead}>{t('setup.doneText')}</p>
              <Choice
                label={t('setup.levelQuestion')}
                variant="cards"
                value={level}
                onValue={setLevel}
                options={[
                  { value: 'simple', label: t('level.simple'), hint: t('level.simpleHint') },
                  { value: 'advanced', label: t('level.advanced'), hint: t('level.advancedHint') },
                ]}
              />
            </>
          )}
          <footer class={s.foot}>
            {i > 0 && (
              <Button variant="ghost" icon={ArrowLeft} onClick={back}>
                {t('setup.back')}
              </Button>
            )}
            {step === 'done' ? (
              <Button variant="primary" size="lg" icon={Check} busy={busy} onClick={() => void finish(level)}>
                {t('setup.finish')}
              </Button>
            ) : (
              <Button type="submit" variant="primary" size="lg" iconEnd={ArrowRight} busy={busy} disabled={step === 'name' && (!name || !!nameError)}>
                {step === 'welcome' ? t('setup.start') : step === 'password' && !pw ? t('setup.noPassword') : t('setup.next')}
              </Button>
            )}
          </footer>
        </form>
      </main>
      <Toasts />
      <ConfirmHost />
    </div>
  );
}
