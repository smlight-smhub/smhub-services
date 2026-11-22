// USB: the port's role (takes effect after a restart) and connected devices.
import { Usb as UsbIcon } from 'lucide-preact';
import { api } from '../../api/client';
import type { Usb as UsbState } from '../../api/types';
import { t } from '../../text/i18n';
import { useGet } from '../../state/get';
import { removeRestartNotice } from '../../state/notices';
import { save } from '../../state/save';
import { features } from '../../state/system';
import { Banner, EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { Choice } from '../../kit/Field';
import { Page, Section } from '../../kit/Page';
import c from '../common.module.css';

export default function Usb() {
  const { data, error, reload, sig } = useGet('/settings/usb');
  const pending: 'device' | 'host' | null = data?.pending_mode && data.pending_mode !== data.mode ? (data.pending_mode === 'host' ? 'host' : 'device') : null;
  const setMode = (mode: 'device' | 'host') =>
    save<UsbState>({
      target: sig,
      optimistic: (u) => ({ ...u, pending_mode: mode === u.mode ? null : mode }),
      send: () => api.patch('/settings/usb', { body: { mode } }),
      success: t('common.saved'),
      restart: (u) => {
        if (!u.pending_mode || u.pending_mode === u.mode) {
          removeRestartNotice('usb-mode');
          return null;
        }
        return { id: 'usb-mode', kind: 'hub', reason: t('restart.usbMode') };
      },
    });
  return (
    <Page title={t('nav.usb')} back={{ href: '/peripherals', label: t('nav.peripherals') }} subtitle={t('nav.usbDesc')} narrow>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={4} />
      ) : (
        <>
          {features.value?.usb_role !== false && (
            <Section title={t('usb.role')} icon={UsbIcon}>
              <Choice
                label={t('usb.role')}
                hideLabel
                variant="cards"
                value={pending ?? data.mode}
                onValue={(v) => void setMode(v)}
                options={[
                  { value: 'device', label: t('usb.device'), hint: t('usb.deviceHint') },
                  { value: 'host', label: t('usb.host'), hint: t('usb.hostHint') },
                ]}
              />
              {pending && <Banner tone="warn">{t('usb.pending', { mode: t(pending === 'host' ? 'usb.host' : 'usb.device') })}</Banner>}
            </Section>
          )}
          <Section title={t('usb.devices')}>
            {data.devices.length ? (
              <ul class={c.list}>
                {data.devices.map((d) => (
                  <li key={d.id + (d.bus ?? '')} class={c.item}>
                    <UsbIcon size={18} class={c.itemIcon} aria-hidden="true" />
                    <div class={c.itemMain}>
                      <span class={c.itemTitle}>{d.name}</span>
                      <span class={`${c.itemMeta} mono`}>
                        {d.id}
                        {d.bus ? ` · ${d.bus}` : ''}
                      </span>
                    </div>
                  </li>
                ))}
              </ul>
            ) : (
              <EmptyState title={t('usb.none')} />
            )}
          </Section>
        </>
      )}
    </Page>
  );
}
