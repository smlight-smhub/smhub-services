// Address settings shared by Ethernet and Wi-Fi: automatic (DHCP) or manual.
import type { IpSettings } from '../../api/types';
import { t } from '../../text/i18n';
import { advanced } from '../../state/prefs';
import { Choice, FieldGrid, ListField, NumberField, Switch, TextField } from '../../kit/Field';

const V4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;
const V6 = /^[0-9a-fA-F:]+$/;

export function isIp(s: string): boolean {
  return V4.test(s) || (s.includes(':') && V6.test(s));
}

export function isCidr(s: string): boolean {
  const [ip, mask, extra] = s.split('/');
  if (!ip || mask === undefined || extra !== undefined || !/^\d+$/.test(mask)) return false;
  const m = Number(mask);
  return V4.test(ip) ? m >= 0 && m <= 32 : isIp(ip) && m >= 0 && m <= 128;
}

/** Field messages for address settings, or an empty object when they are fine. */
export function checkIp(v: IpSettings): Record<string, string> {
  const e: Record<string, string> = {};
  if (v.method !== 'manual') return e;
  const addrs = (v.addresses ?? []).filter(Boolean);
  if (!addrs.length) e.addresses = t('ip.needAddress');
  else if (addrs.some((a) => !isCidr(a))) e.addresses = t('ip.badCidr');
  if (v.gateway && !isIp(v.gateway)) e.gateway = t('ip.badIp');
  if ((v.dns ?? []).filter(Boolean).some((d) => !isIp(d))) e.dns = t('ip.badIp');
  if (v.mtu !== null && v.mtu !== undefined && (v.mtu < 576 || v.mtu > 9000)) e.mtu = t('form.range', { min: 576, max: 9000 });
  return e;
}

/** Drops empty rows before sending. */
export function cleanIp(v: IpSettings): IpSettings {
  return { ...v, addresses: (v.addresses ?? []).filter(Boolean), dns: (v.dns ?? []).filter(Boolean), gateway: v.gateway || null };
}

export function IpFields({ value, onChange, errors }: { value: IpSettings; onChange: (v: IpSettings) => void; errors: Record<string, string> }) {
  const set = <K extends keyof IpSettings>(k: K, v: IpSettings[K]) => onChange({ ...value, [k]: v });
  const manual = value.method === 'manual';
  return (
    <>
      <Choice
        label={t('ip.method')}
        variant="cards"
        value={value.method ?? 'auto'}
        onValue={(m) => set('method', m)}
        options={[
          { value: 'auto', label: t('ip.auto'), hint: t('ip.autoHint') },
          { value: 'manual', label: t('ip.manual'), hint: t('ip.manualHint') },
        ]}
      />
      {manual && (
        <>
          <ListField label={t('ip.addresses')} hint={t('ip.addressesHint')} placeholder="192.168.1.20/24" value={value.addresses ?? []} onValue={(v) => set('addresses', v)} error={errors.addresses} />
          <FieldGrid>
            <TextField label={t('ip.gateway')} mono value={value.gateway ?? ''} onValue={(v) => set('gateway', v)} placeholder="192.168.1.1" error={errors.gateway} inputMode="decimal" />
          </FieldGrid>
        </>
      )}
      {(manual || advanced.value) && (
        <ListField label={t('ip.dns')} hint={manual ? t('ip.dnsHint') : t('ip.dnsAutoHint')} placeholder="1.1.1.1" value={value.dns ?? []} onValue={(v) => set('dns', v)} error={errors.dns} />
      )}
      {advanced.value && (
        <>
          <NumberField label={t('ip.mtu')} hint={t('ip.mtuHint')} min={576} max={9000} value={value.mtu} onValue={(v) => set('mtu', v)} error={errors.mtu} />
          <Switch label={t('ip.autoconnect')} description={t('ip.autoconnectDesc')} checked={value.autoconnect ?? true} onChange={(v) => set('autoconnect', v)} />
        </>
      )}
    </>
  );
}
