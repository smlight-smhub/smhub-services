// UPS board (full-size SMHUB): the owner marks an SMLIGHT UPS board on J10 as connected (nothing
// on the board tells the hub reliably), and the hub turns the board's I2C bus on from the next
// start; experts can scan it. docs/peripherals.md, "UPS board".
import { useState } from 'preact/hooks';
import { BatteryCharging, ScanSearch } from 'lucide-preact';
import { api } from '../../api/client';
import type { Ups as UpsState } from '../../api/types';
import { t } from '../../text/i18n';
import { list } from '../../text/format';
import { useGet } from '../../state/get';
import { removeRestartNotice } from '../../state/notices';
import { advanced } from '../../state/prefs';
import { errorText, save } from '../../state/save';
import { toast } from '../../state/toasts';
import { Button } from '../../kit/Button';
import { Banner, ErrorState, Loading } from '../../kit/Feedback';
import { Switch } from '../../kit/Field';
import { Page, Section } from '../../kit/Page';
import { Dot } from '../../kit/Status';

const hex = (a: number) => `0x${a.toString(16).toUpperCase().padStart(2, '0')}`;

function Scan() {
  const [found, setFound] = useState<number[] | null>(null);
  const [busy, setBusy] = useState(false);
  const scan = async () => {
    setBusy(true);
    try {
      setFound((await api.post('/ups/scan')).addresses);
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Button icon={ScanSearch} busy={busy} onClick={() => void scan()}>
        {t('ups.scan')}
      </Button>
      {found && (
        <p role="status" class="mono">
          {found.length ? t('ups.found', { list: list(found.map(hex), 'unit') }) : t('ups.nothing')}
        </p>
      )}
    </>
  );
}

export default function Ups() {
  const { data, error, reload, sig } = useGet('/ups');
  const setInstalled = (installed: boolean) =>
    save<UpsState>({
      target: sig,
      optimistic: (u) => ({ ...u, installed }),
      send: () => api.patch('/ups', { body: { installed } }),
      success: t('common.saved'),
      restart: (u) => {
        if (!u.restart_needed) {
          removeRestartNotice('ups');
          return null;
        }
        return { id: 'ups', kind: 'hub', reason: t('restart.ups') };
      },
    });
  return (
    <Page title={t('nav.ups')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.upsDesc')} narrow>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={3} />
      ) : (
        <Section title={t('ups.title')} icon={BatteryCharging}>
          <Switch label={t('ups.installed')} description={t('ups.installedHint')} checked={data.installed} onChange={(v) => void setInstalled(v)} />
          {data.restart_needed ? (
            <Banner tone="info">{data.installed ? t('ups.afterRestartOn') : t('ups.afterRestartOff')}</Banner>
          ) : (
            data.installed && <Dot tone="ok" label={t('ups.busReady', { bus: `/dev/i2c-${data.bus ?? 2}` })} />
          )}
          {advanced.value && data.installed && data.bus !== null && <Scan />}
        </Section>
      )}
    </Page>
  );
}
