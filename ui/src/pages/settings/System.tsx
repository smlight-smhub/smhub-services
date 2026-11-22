// System: what this hub is, and restarting or powering it off (with confirmation).
import { LocateFixed, Power, RotateCw, Server, Square } from 'lucide-preact';
import { t } from '../../text/i18n';
import { dateTime, duration } from '../../text/format';
import { findHub } from '../../state/hardware';
import { powerOffHub, restartHub } from '../../state/power';
import { useLive } from '../../state/store';
import { advanced } from '../../state/prefs';
import { hubTime, system, uptimeAt, useNow } from '../../state/system';
import { Button } from '../../kit/Button';
import { CopyValue } from '../../kit/Bits';
import { Loading } from '../../kit/Feedback';
import { Facts, Page, Section } from '../../kit/Page';
import { Logo } from '../../shell/Logo';
import s from './Settings.module.css';

export default function System() {
  const sys = system.value;
  const hw = useLive('hardware');
  const now = useNow('minute');
  const up = uptimeAt(now);
  return (
    <Page title={t('nav.system')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.systemDesc')} narrow>
      {!sys ? (
        <Loading lines={6} />
      ) : (
        <>
          <Section>
            <div class={s.hero}>
              <Logo />
              <div>
                <p>
                  <strong>{sys.product}</strong>
                </p>
                <p class="muted">{sys.hostname}</p>
              </div>
            </div>
            <Facts
              items={[
                [t('sys.model'), sys.model],
                [t('sys.hardware'), sys.hardware_revision],
                [t('sys.os'), <span class="num">{sys.os_version}</span>],
                [t('sys.core'), <span class="num">{sys.core_version}</span>],
                [t('sys.uptime'), up !== undefined ? duration(up, 'long') : null],
                [t('sys.time'), dateTime(hubTime(now))],
                [t('sys.timezone'), sys.timezone],
                [t('sys.deviceId'), advanced.value && sys.device_id && <CopyValue value={sys.device_id} label={t('common.copy')} />],
              ]}
            />
          </Section>
          {hw && (hw.buzzer.available || hw.light.available) && (
            <Section id="find" title={t('find.title')} icon={LocateFixed} description={t('find.desc')}>
              <div class={s.powerRow}>
                {hw.find ? (
                  <Button variant="primary" icon={Square} onClick={() => void findHub(false)}>
                    {t('find.stop')}
                  </Button>
                ) : (
                  <Button icon={LocateFixed} onClick={() => void findHub(true)}>
                    {t('find.start')}
                  </Button>
                )}
              </div>
            </Section>
          )}
          <Section id="power" title={t('system.power')} icon={Server} description={t('system.powerDesc')}>
            <div class={s.powerRow}>
              <Button icon={RotateCw} onClick={() => void restartHub()}>
                {t('power.restart')}
              </Button>
              <Button variant="danger" icon={Power} onClick={() => void powerOffHub()}>
                {t('power.off')}
              </Button>
            </div>
          </Section>
        </>
      )}
    </Page>
  );
}
