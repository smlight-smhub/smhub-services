// Radios: one card per slot. Flashing is a guided flow: choose what the
// radio should do, pick firmware, confirm, and watch the progress live.
import { useEffect, useState } from 'preact/hooks';
import { CircleCheck, Cpu, Radio as RadioIcon, TriangleExclamationPoint, Zap } from 'lucide-preact';
import { api } from '../../api/client';
import type { Firmware, Radio, RadioMode } from '../../api/types';
import { t } from '../../text/i18n';
import { operationById, operationStep, startOperation, latestOperation } from '../../state/operations';
import { advanced } from '../../state/prefs';
import { errorText } from '../../state/save';
import { useLive } from '../../state/store';
import { Button } from '../../kit/Button';
import { appTitle } from '../../kit/Bits';
import { Card } from '../../kit/Card';
import { Dialog } from '../../kit/Dialog';
import { Banner, Loading, ProgressBar } from '../../kit/Feedback';
import { Choice, TextField } from '../../kit/Field';
import { Markdown } from '../../kit/Markdown';
import { Facts, Page } from '../../kit/Page';
import { Dot } from '../../kit/Status';
import c from '../common.module.css';

const IEEE = /^[0-9A-Fa-f]{16}$/;

function FlashDialog({ radio, onClose }: { radio: Radio | null; onClose: () => void }) {
  const [mode, setMode] = useState<RadioMode | undefined>();
  const [list, setList] = useState<Firmware[] | null>(null);
  const [listError, setListError] = useState<string>();
  const [version, setVersion] = useState<string>();
  const [ieee, setIeee] = useState('');
  const [opId, setOpId] = useState<string>();
  const [starting, setStarting] = useState(false);
  /** The app that used the radio when the flash started: the hub stops it and starts it again. */
  const [hadApp, setHadApp] = useState<string | null>(null);
  const op = operationById(opId);

  useEffect(() => {
    setMode(radio?.firmware?.mode ?? radio?.modes[0]);
    setOpId(undefined);
    setIeee('');
  }, [radio]);
  useEffect(() => {
    if (!radio || !mode) return;
    setList(null);
    setVersion(undefined);
    setListError(undefined);
    api
      .get('/radios/{index}/firmware', { path: { index: radio.index }, query: { mode } })
      .then((l) => {
        setList(l);
        setVersion(l.find((f) => !f.dev)?.version ?? l[0]?.version);
      })
      .catch((e) => setListError(errorText(e)));
  }, [radio, mode]);

  if (!radio) return <Dialog open={false} onClose={onClose} title="" />;
  const zigbee = mode === 'zigbee_coordinator' || mode === 'zigbee_router';
  const ieeeError = ieee && !IEEE.test(ieee) ? t('radio.ieeeInvalid') : undefined;
  const chosen = list?.find((f) => f.version === version);
  const running = op?.state === 'running';
  const flash = async () => {
    if (!mode || !version || ieeeError) return;
    setHadApp(radio.in_use_by ?? null);
    setStarting(true);
    const o = await startOperation(() => api.post('/radios/{index}/flash', { path: { index: radio.index }, body: { mode, version, ieee: zigbee && ieee ? ieee : null } }));
    setStarting(false);
    if (o) setOpId(o.id);
  };

  return (
    <Dialog
      open={!!radio}
      onClose={onClose}
      size="lg"
      title={t('radio.flashTitle', { chip: radio.chip })}
      footer={
        op ? (
          <Button variant={running ? 'secondary' : 'primary'} onClick={onClose}>
            {running ? t('radio.hide') : t('common.done')}
          </Button>
        ) : (
          <>
            <Button onClick={onClose}>{t('common.cancel')}</Button>
            <Button variant="primary" icon={Zap} busy={starting} disabled={!version || !!ieeeError} onClick={() => void flash()}>
              {t('radio.flash')}
            </Button>
          </>
        )
      }
    >
      {op ? (
        <div class={c.form} role="status">
          {op.state === 'succeeded' ? (
            <Banner tone="ok" icon={CircleCheck} title={t('radio.flashDone')}>
              {hadApp ? t('radio.flashDoneText') : t('radio.flashDoneFree')}
            </Banner>
          ) : op.state === 'failed' ? (
            <Banner tone="danger" title={t('radio.flashFailed')}>
              {op.error ?? ''}
            </Banner>
          ) : (
            <>
              <p>
                <strong>{t('radio.flashing', { version: version ?? '' })}</strong>
              </p>
              <ProgressBar value={op.progress} label={t('radio.flashing', { version: version ?? '' })} />
              {operationStep(op) && <p class={c.note}>{operationStep(op)}</p>}
              <Banner tone="warn">{t('radio.dontUnplug')}</Banner>
            </>
          )}
        </div>
      ) : (
        <>
          <Choice
            label={t('radio.mode')}
            variant="cards"
            value={mode}
            onValue={setMode}
            options={radio.modes.map((m) => ({ value: m, label: t(`radio.mode.${m}`), hint: t(`radio.modeHint.${m}`) }))}
          />
          {listError ? (
            <Banner tone="danger">{listError}</Banner>
          ) : !list ? (
            <Loading lines={3} />
          ) : !list.length ? (
            <Banner tone="info">{t('radio.noFirmware')}</Banner>
          ) : (
            <Choice
              label={t('radio.firmware')}
              variant="cards"
              value={version}
              onValue={setVersion}
              options={list
                .filter((f) => advanced.value || !f.dev || f.version === version)
                .map((f, i) => ({ value: f.version, label: f.version, hint: f.dev ? t('radio.devBuild') : i === 0 ? t('radio.newest') : undefined }))}
            />
          )}
          {chosen?.notes && (
            <details class={c.details}>
              <summary>{t('radio.notes')}</summary>
              <div class={c.detailsBody}>
                <Markdown text={chosen.notes} />
              </div>
            </details>
          )}
          {zigbee && advanced.value && (
            <TextField label={t('radio.ieee')} hint={t('radio.ieeeHint')} mono value={ieee} onValue={setIeee} maxLength={16} error={ieeeError} placeholder="00124B0012345678" />
          )}
          <Banner tone="warn" icon={TriangleExclamationPoint} title={t('radio.warnTitle')}>
            {radio.in_use_by ? t('radio.warnInUse', { app: appTitle(radio.in_use_by) }) : t('radio.warnFree')}
          </Banner>
        </>
      )}
    </Dialog>
  );
}

export default function Radios() {
  const r = useLive('radios');
  const [flashing, setFlashing] = useState<Radio | null>(null);
  return (
    <Page title={t('nav.radios')} subtitle={t('nav.radiosDesc')}>
      {!r ? (
        <Loading lines={6} />
      ) : (
        <div class={c.grid}>
          {r.radios.map((radio) => {
            const op = latestOperation(['radio_flash'], String(radio.index));
            const busy = op?.state === 'running';
            return (
              <Card
                key={radio.index}
                title={radio.chip}
                icon={RadioIcon}
                action={radio.in_use_by ? <Dot tone="ok" label={t('radio.usedBy', { app: appTitle(radio.in_use_by) })} /> : <Dot tone="neutral" label={t('radio.free')} />}
                footer={
                  <Button variant="primary" icon={Zap} disabled={busy} onClick={() => setFlashing(radio)}>
                    {t('radio.update')}
                  </Button>
                }
              >
                <Facts
                  items={[
                    [t('radio.slot'), String(radio.index)],
                    [t('radio.protocols'), radio.protocols.map((p) => t(`radio.protocol.${p}`)).join(', ')],
                    [t('radio.currentMode'), radio.firmware?.mode ? t(`radio.mode.${radio.firmware.mode}`) : t('radio.unknownFirmware')],
                    [t('radio.version'), radio.firmware?.version && <span class="num">{radio.firmware.version}</span>],
                    [t('radio.ieee'), advanced.value && radio.firmware?.ieee && <span class="mono">{radio.firmware.ieee}</span>],
                    [t('radio.port'), advanced.value && <span class="mono">{radio.port}</span>],
                    [t('radio.vendor'), advanced.value && <span class={c.inline}><Cpu size={14} aria-hidden="true" />{radio.vendor === 'ti' ? 'Texas Instruments' : 'Silicon Labs'}</span>],
                    [t('radio.key'), advanced.value && <span class="mono">{radio.key}</span>],
                  ]}
                />
                {busy && op && <ProgressBar value={op.progress} label={t('radio.flashing', { version: '' })} />}
              </Card>
            );
          })}
        </div>
      )}
      <FlashDialog radio={flashing} onClose={() => setFlashing(null)} />
    </Page>
  );
}
