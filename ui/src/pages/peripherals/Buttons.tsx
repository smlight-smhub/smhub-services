// The hub's buttons, as on the case from the left: BTN1 (full-size SMHUB), BTN2 (every model, the
// Nanos' only one; held 10 s it resets the hub to factory settings) and RST (full-size SMHUB;
// restarts the chip, no software sees it). Their live state, the last gesture, and what a short,
// double or long press does.
import { useEffect, useState } from 'preact/hooks';
import { CircleDot, Hand } from 'lucide-preact';
import { api } from '../../api/client';
import type { ButtonAction, ButtonSettings, ButtonState, HardwareSettings } from '../../api/types';
import { t, tx } from '../../text/i18n';
import { relative, seconds, time } from '../../text/format';
import { useGet } from '../../state/get';
import { save, useFieldErrors } from '../../state/save';
import { useLive } from '../../state/store';
import { system } from '../../state/system';
import { Banner, ErrorState, Loading } from '../../kit/Feedback';
import { SelectField, Switch, TextField } from '../../kit/Field';
import { Page, Rows, Section } from '../../kit/Page';
import { Dot } from '../../kit/Status';
import { cx } from '../../kit/cx';
import s from './Buttons.module.css';

type Gesture = 'short' | 'double' | 'long';
type Which = 'button' | 'button2';
const GESTURES: Gesture[] = ['short', 'double', 'long'];
const ACTIONS: ButtonAction['action'][] = ['none', 'stop_sound', 'toggle_light', 'toggle_leds', 'play_sound', 'send_ir', 'call_url'];
const NAME: Record<Which, string> = { button: 'BTN1', button2: 'BTN2' };

/** The address a gesture calls: edited freely, saved when the field is left. */
function UrlField({ value, onSave }: { value: string; onSave: (url: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <TextField
      label={t('btn.url')}
      hint={t('btn.urlHint')}
      type="url"
      value={draft}
      onValue={setDraft}
      onBlur={() => {
        if (draft.trim() !== value) onSave(draft.trim());
      }}
    />
  );
}

/** One button drawn as a key cap: lit while it is held. Without a state, the hub cannot see it. */
function Cap({ name, state }: { name: string; state?: ButtonState }) {
  const down = !!state?.pressed;
  const label = `${name}: ${state ? (down ? t('btn.pressed') : t('btn.released')) : t('btn.notSeen')}`;
  return (
    <span class={cx(s.cap, down && s.down, !state && s.inert)} role="img" aria-label={label}>
      {name}
    </span>
  );
}

/** The last gesture; a long press says how long it was held, not the setting's threshold. */
function lastLine(b: ButtonState) {
  const l = b.last;
  if (!l) return t('btn.noneYet');
  const g = l.gesture === 'long' ? t('btn.g.longHeld', { time: seconds(l.held_ms / 1000) }) : t(`btn.g.${l.gesture as Gesture}`);
  return t('btn.last', { gesture: g, when: `${time(l.at, true)} (${relative(l.at)})` });
}

export default function Buttons() {
  const hw = useLive('hardware');
  const ir = useLive('ir');
  const set = useGet('/settings/hardware');
  const errors = useFieldErrors();
  const settings = set.data;
  // RST, a button that restarts the chip, is reachable from outside only on the full-size SMHUB
  // (the Nano MR's sits inside the case, the Nano has only a header)
  const full = system.value?.family === 'smhub';
  const patch = (which: Which, body: Partial<Record<Gesture, Partial<ButtonAction>>> | { feedback: boolean }) =>
    void save<HardwareSettings>({
      target: set.sig,
      send: () => api.patch('/settings/hardware', { body: { [which]: body } }),
      errors,
      success: t('common.saved'),
    });
  const sounds = hw ? [...hw.presets.map((p) => ({ value: p, label: tx(`hw.preset.${p}`, p) })), ...(settings?.sounds.melodies ?? []).map((m) => ({ value: m.name, label: m.name }))] : [];
  const codes = (ir?.codes ?? []).map((c) => ({ value: c.id, label: c.device ? `${c.device} · ${c.name}` : c.name }));
  const actionAvailable = (a: ButtonAction['action']) => {
    if (!hw) return true;
    if (a === 'stop_sound' || a === 'play_sound') return hw.buzzer.available;
    if (a === 'toggle_light') return hw.light.available;
    if (a === 'toggle_leds') return hw.leds.available;
    if (a === 'send_ir') return hw.ir.available;
    return true;
  };

  const actions = (which: Which, b: ButtonSettings) => (
    <Section key={which} id={which === 'button' ? 'btn1' : 'btn2'} title={t('btn.actionsFor', { name: NAME[which] })} icon={Hand} description={t('btn.actionsDesc')}>
      {which === 'button2' && <Banner tone="info">{t('btn.btn2Note')}</Banner>}
      {GESTURES.map((g) => {
        const a = b[g];
        return (
          <div key={g} class={s.gesture}>
            <SelectField
              label={g === 'long' && which === 'button2' ? t('btn.g.long2') : t(`btn.g.${g}`)}
              value={a.action}
              error={errors.value[`${which}.${g}`]}
              onValue={(v) => {
                const next: Partial<ButtonAction> = { action: v };
                if (v === 'play_sound') next.preset = a.preset ?? sounds[0]?.value ?? null;
                if (v === 'send_ir') next.code = a.code ?? codes[0]?.value ?? null;
                if (v === 'call_url') next.url = a.url ?? 'http://';
                patch(which, { [g]: next });
              }}
              options={ACTIONS.map((x) => ({ value: x, label: t(`btn.a.${x}`), disabled: !actionAvailable(x) }))}
            />
            {a.action === 'play_sound' && <SelectField label={t('btn.sound')} value={a.preset ?? ''} onValue={(v) => patch(which, { [g]: { preset: v } })} options={sounds} />}
            {a.action === 'send_ir' && (codes.length ? <SelectField label={t('btn.code')} value={a.code ?? ''} onValue={(v) => patch(which, { [g]: { code: v } })} options={codes} /> : <p class="muted">{t('btn.noCodes')}</p>)}
            {a.action === 'call_url' && <UrlField value={a.url ?? ''} onSave={(url) => patch(which, { [g]: { url } })} />}
          </div>
        );
      })}
      {hw?.buzzer.available && (
        <Rows>
          <Switch label={t('btn.feedback')} description={t('btn.feedbackDesc')} checked={b.feedback} onChange={(v) => patch(which, { feedback: v })} />
        </Rows>
      )}
    </Section>
  );

  return (
    <Page title={t('nav.buttons')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.buttonsDesc')} narrow>
      {set.error && !settings ? (
        <ErrorState error={set.error} retry={set.reload} />
      ) : !hw || !settings ? (
        <Loading lines={6} />
      ) : !hw.button.available && !hw.button2.available ? (
        <Banner tone="info">{t('btn.unavailable')}</Banner>
      ) : (
        <>
          <Section id="state" title={t('btn.state')} icon={CircleDot}>
            <div class={s.panel}>
              {hw.button.available && <Cap name="BTN1" state={hw.button} />}
              {hw.button2.available && <Cap name="BTN2" state={hw.button2} />}
              {full && <Cap name="RST" />}
            </div>
            <ul class={s.states} aria-live="polite">
              {(['button', 'button2'] as Which[])
                .filter((w) => hw[w].available)
                .map((w) => (
                  <li key={w}>
                    <strong>{NAME[w]}</strong>
                    <Dot tone={hw[w].pressed ? 'ok' : 'neutral'} label={hw[w].pressed ? t('btn.pressed') : t('btn.released')} />
                    <span class="muted">{lastLine(hw[w])}</span>
                  </li>
                ))}
              {full && (
                <li>
                  <strong>RST</strong>
                  <span class="muted">{t('btn.rstNote')}</span>
                </li>
              )}
            </ul>
            <p class="muted">{t('btn.tryIt')}</p>
          </Section>
          {hw.button.available && actions('button', settings.button)}
          {hw.button2.available && actions('button2', settings.button2)}
        </>
      )}
    </Page>
  );
}
