// The parts of the Buzzer and Lights pages (docs/peripherals.md): the buzzer's sounds and event
// sounds, the ambient light ring, the service LEDs, and the recent activity. Everything is live:
// what plays, what the ring shows (also when Home Assistant changed it), the LEDs. Every new sound
// or light feature is off until the user turns it on.
import { useState } from 'preact/hooks';
import { BellRing, Lightbulb, Moon, Play, Plus, Square, Trash, Volume2 } from 'lucide-preact';
import { api } from '../../api/client';
import type { Hardware, HardwareSettings, Owner } from '../../api/types';
import { t, tx } from '../../text/i18n';
import { percent, relative } from '../../text/format';
import { useGet } from '../../state/get';
import { play, setLeds, setLight, stopSound } from '../../state/hardware';
import { advanced } from '../../state/prefs';
import { save, useFieldErrors } from '../../state/save';
import { useLive } from '../../state/store';
import { Button, IconButton } from '../../kit/Button';
import { Banner } from '../../kit/Feedback';
import { SelectField, Switch, TextArea, TextField } from '../../kit/Field';
import { Actions, Rows, Section } from '../../kit/Page';
import { Slider } from '../../kit/Slider';
import { Status } from '../../kit/Status';
import c from '../common.module.css';
import s from './Hardware.module.css';

export type Patch = (p: Record<string, unknown>, msg?: string) => void;

/** The live hardware, its settings, and a way to change them: what both pages need. */
export function useHardware() {
  const hw = useLive('hardware');
  const set = useGet('/settings/hardware');
  const errors = useFieldErrors();
  const patch: Patch = (body, message) =>
    void save<HardwareSettings>({
      target: set.sig,
      send: () => api.patch('/settings/hardware', { body }),
      errors,
      success: message ?? t('common.saved'),
    });
  return { hw, set, settings: set.data, errors, patch };
}

const SWATCHES = ['#ffb45a', '#ffffff', '#ff0000', '#ff8000', '#ffd000', '#00ff40', '#00ffff', '#0040ff', '#8000ff', '#ff0080'];

/** A device's driver, for experts: "Run by ESPHome" or "Run by the hub". */
export function OwnerNote({ owner }: { owner: Owner }) {
  if (!advanced.value) return null;
  return <p class="muted">{tx(`hw.owner.${owner}`, owner)}</p>;
}

/** The hub names effects the same for every owner; an effect it does not know shows as named. */
export function effectName(e: string): string {
  return tx(`hw.effect.${e.replace(/\s+/g, '')}`, e);
}

/** ESPHome is not answering while it runs devices shown on this page. */
export function EspNotice({ hw, owners }: { hw: Hardware; owners: Owner[] }) {
  const uses = owners.includes('esphome');
  if (!uses || hw.esphome.connected) return null;
  return (
    <Banner tone="warn" title={t('hw.espDownTitle')}>
      {t('hw.espDown')}
      {advanced.value && hw.esphome.error ? ` (${hw.esphome.error})` : ''}
    </Banner>
  );
}

export function Sounds({ hw, settings, patch }: { hw: Hardware; settings: HardwareSettings; patch: Patch }) {
  const [rtttl, setRtttl] = useState('');
  const [name, setName] = useState('');
  const b = hw.buzzer;
  const melodies = settings.sounds.melodies;
  const addMelody = () => {
    if (!name.trim() || !rtttl.trim()) return;
    patch({ sounds: { melodies: [...melodies, { name: name.trim(), rtttl: rtttl.trim() }] } }, t('hw.melodySaved', { name: name.trim() }));
    setName('');
  };
  return (
    <Section
      id="sounds"
      title={t('hw.sounds')}
      icon={BellRing}
      description={t('hw.soundsDesc')}
      actions={
        b.playing && (
          <Button size="sm" icon={Square} onClick={() => void stopSound()}>
            {t('hw.stop')}
          </Button>
        )
      }
    >
      <OwnerNote owner={b.owner} />
      {b.playing && (
        <p aria-live="polite">
          <Status tone="busy" size="sm">
            {t('hw.playing', { name: tx(`hw.preset.${b.playing.label}`, b.playing.label), by: b.playing.by })}
          </Status>
          {b.queue.length > 0 && <span class="muted"> · {t('hw.waiting', { count: b.queue.length })}</span>}
        </p>
      )}
      <div class={s.presets}>
        {hw.presets.map((p) => (
          <Button key={p} icon={Play} onClick={() => void play({ sound: p })}>
            {tx(`hw.preset.${p}`, p)}
          </Button>
        ))}
        {melodies.map((m) => (
          <Button key={m.name} icon={Play} variant="ghost" onClick={() => void play({ sound: m.name })}>
            {m.name}
          </Button>
        ))}
      </div>
      {advanced.value && (
        <details class={c.details}>
          <summary>{t('hw.ownMelody')}</summary>
          <div class={c.detailsBody}>
            <TextField label={t('hw.melodyName')} value={name} onValue={setName} maxLength={40} />
            <TextArea label={t('hw.rtttl')} hint={t('hw.rtttlHint')} value={rtttl} onValue={setRtttl} rows={3} mono maxLength={512} />
            <Actions>
              <Button icon={Play} disabled={!rtttl.trim()} onClick={() => void play({ rtttl: rtttl.trim() })}>
                {t('hw.try')}
              </Button>
              <Button icon={Plus} variant="primary" disabled={!rtttl.trim() || !name.trim()} onClick={addMelody}>
                {t('hw.saveMelody')}
              </Button>
            </Actions>
            {melodies.length > 0 && (
              <ul class={c.list}>
                {melodies.map((m) => (
                  <li key={m.name} class={c.item}>
                    <div class={c.itemMain}>
                      <span class={c.itemTitle}>{m.name}</span>
                      <span class={`${c.itemMeta} mono`}>{m.rtttl}</span>
                    </div>
                    <span class={c.itemActions}>
                      <IconButton
                        icon={Trash}
                        label={t('hw.deleteMelody', { name: m.name })}
                        onClick={() => patch({ sounds: { melodies: melodies.filter((x) => x.name !== m.name) } }, t('hw.melodyDeleted', { name: m.name }))}
                      />
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </details>
      )}
    </Section>
  );
}

export function EventSounds({ settings, patch }: { settings: HardwareSettings; patch: Patch }) {
  const e = settings.sounds.events;
  const q = settings.sounds.quiet;
  const events: [keyof typeof e, string][] = [
    ['update_done', t('hw.ev.updateDone')],
    ['failed', t('hw.ev.failed')],
    ['crashed', t('hw.ev.crashed')],
    ['backup_done', t('hw.ev.backupDone')],
    ['started', t('hw.ev.started')],
    ['power', t('hw.ev.power')],
  ];
  return (
    <Section id="events" title={t('hw.eventSounds')} icon={Volume2} description={t('hw.eventSoundsDesc')}>
      <Rows>
        {events.map(([k, label]) => (
          <Switch key={k} label={label} checked={e[k]} onChange={(v) => patch({ sounds: { events: { [k]: v } } })} />
        ))}
        <Switch label={t('hw.mute')} description={t('hw.muteDesc')} checked={settings.sounds.mute} onChange={(v) => patch({ sounds: { mute: v } })} />
        <Switch label={t('hw.quiet')} description={t('hw.quietDesc')} checked={q.enabled} onChange={(v) => patch({ sounds: { quiet: { enabled: v } } })} />
      </Rows>
      {q.enabled && (
        <div class={s.times}>
          <TextField type="time" label={t('hw.from')} value={q.from} onValue={(v) => v && patch({ sounds: { quiet: { from: v } } })} />
          <TextField type="time" label={t('hw.to')} value={q.to} onValue={(v) => v && patch({ sounds: { quiet: { to: v } } })} />
        </div>
      )}
    </Section>
  );
}

export function LightCard({ hw, settings, patch }: { hw: Hardware; settings: HardwareSettings; patch: Patch }) {
  const l = hw.light;
  const [bri, setBri] = useState<number | null>(null);
  const [speed, setSpeed] = useState<number | null>(null);
  const busy = !l.known && l.owner === 'esphome';
  return (
    <Section
      id="light"
      title={t('hw.light')}
      icon={Lightbulb}
      description={t('hw.lightDesc')}
      actions={l.signal && <Status tone="info" size="sm">{tx(`hw.signal.${l.signal}`, l.signal)}</Status>}
    >
      <OwnerNote owner={l.owner} />
      {busy && <p class="muted">{t('hw.lightUnknown')}</p>}
      {hw.dark && <Banner tone="info" icon={Moon}>{t('hw.darkNow')}</Banner>}
      <Rows>
        <Switch label={t('hw.lightOn')} checked={l.on} disabled={busy} onChange={(v) => void setLight({ on: v })} />
      </Rows>
      <div class={s.swatches} role="group" aria-label={t('hw.color')}>
        {SWATCHES.map((col) => (
          <button
            key={col}
            type="button"
            class={s.swatch}
            style={{ background: col }}
            aria-label={t('hw.colorNamed', { color: col })}
            aria-pressed={l.color === col}
            disabled={busy}
            onClick={() => void setLight({ color: col })}
          />
        ))}
        <label class={s.picker}>
          <span class="visually-hidden">{t('hw.colorOther')}</span>
          <input type="color" value={l.color} disabled={busy} onChange={(e) => void setLight({ color: (e.currentTarget as HTMLInputElement).value })} />
        </label>
      </div>
      <Slider
        label={t('hw.brightness')}
        value={bri ?? l.brightness}
        min={1}
        max={100}
        valueText={percent(bri ?? l.brightness)}
        disabled={busy}
        onInput={setBri}
        onValue={(v) => {
          setBri(null);
          void setLight({ brightness: v });
        }}
      />
      <SelectField
        label={t('hw.effect')}
        value={l.effect}
        disabled={busy}
        onValue={(v) => void setLight({ effect: v })}
        options={[{ value: 'None', label: t('hw.effectNone') }, ...l.effects.map((e) => ({ value: e, label: effectName(e) }))]}
      />
      {l.speed !== null && l.effect !== 'None' && (
        <Slider
          label={t('hw.speed')}
          value={speed ?? l.speed}
          min={1}
          max={100}
          valueText={percent(speed ?? l.speed)}
          onInput={setSpeed}
          onValue={(v) => {
            setSpeed(null);
            void setLight({ speed: v });
          }}
        />
      )}
      {!l.per_led && advanced.value && <p class="muted">{t('hw.firmwareLimits')}</p>}
      <Rows>
        <Switch label={t('hw.signals')} description={t('hw.signalsDesc')} checked={settings.light.signals} onChange={(v) => patch({ light: { signals: v } })} />
      </Rows>
    </Section>
  );
}

export function Leds({ hw, settings, patch }: { hw: Hardware; settings: HardwareSettings; patch: Patch }) {
  const l = hw.leds;
  const n = settings.leds.night;
  const busy = !l.known && l.owner === 'esphome';
  return (
    <Section id="leds" title={t('hw.leds')} icon={Moon} description={t('hw.ledsDesc')}>
      <OwnerNote owner={l.owner} />
      <Rows>
        <Switch label={t('hw.powerLed')} checked={l.power ?? true} disabled={busy} onChange={(v) => void setLeds({ power: v })} />
        <Switch label={t('hw.blueLed')} description={t('hw.blueLedDesc')} checked={l.blue ?? false} disabled={busy} onChange={(v) => void setLeds({ blue: v })} />
        <Switch label={t('hw.allOff')} description={t('hw.allOffDesc')} checked={settings.leds.all_off} onChange={(v) => patch({ leds: { all_off: v } })} />
        <Switch label={t('hw.night')} description={t('hw.nightDesc')} checked={n.enabled} onChange={(v) => patch({ leds: { night: { enabled: v } } })} />
      </Rows>
      {n.enabled && (
        <div class={s.times}>
          <TextField type="time" label={t('hw.from')} value={n.from} onValue={(v) => v && patch({ leds: { night: { from: v } } })} />
          <TextField type="time" label={t('hw.to')} value={n.to} onValue={(v) => v && patch({ leds: { night: { to: v } } })} />
        </div>
      )}
    </Section>
  );
}

type ActivityKind = Hardware['activity'][number]['kind'];

/** Recent activity of the kinds this page is about (advanced level). */
export function ActivityList({ hw, kinds }: { hw: Hardware; kinds: ActivityKind[] }) {
  if (!advanced.value) return null;
  const items = hw.activity.filter((a) => kinds.includes(a.kind));
  return (
    <Section id="activity" title={t('hw.activity')} description={t('hw.activityDesc')}>
      {items.length === 0 ? (
        <p class="muted">{t('hw.noActivity')}</p>
      ) : (
        <ul class={c.list}>
          {items.slice(0, 30).map((a, i) => (
            <li key={i} class={c.item}>
              <div class={c.itemMain}>
                <span class={c.itemTitle}>
                  {tx(`hw.kind.${a.kind}`, a.kind)}: {a.detail}
                </span>
                <span class={c.itemMeta}>
                  {a.by} · {relative(a.at)} · {tx(`hw.result.${a.result}`, a.result)}
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

