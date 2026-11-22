// Status always uses colour, an icon and text together (never colour alone).
import type { ComponentChildren } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { CircleCheck, CircleDot, CircleExclamationPoint, CircleStop, CircleX, Info, LoaderCircle, TriangleExclamationPoint } from 'lucide-preact';
import type { ServiceState } from '../api/types';
import { t } from '../text/i18n';
import { cx } from './cx';
import s from './Status.module.css';

export type StatusTone = 'ok' | 'warn' | 'danger' | 'info' | 'neutral' | 'busy';

const ICONS: Record<StatusTone, LucideIcon> = {
  ok: CircleCheck,
  warn: TriangleExclamationPoint,
  danger: CircleX,
  info: Info,
  neutral: CircleDot,
  busy: LoaderCircle,
};

export function Status({ tone, children, icon, size = 'md', class: c }: { tone: StatusTone; children: ComponentChildren; icon?: LucideIcon; size?: 'sm' | 'md'; class?: string }) {
  const I = icon ?? ICONS[tone];
  return (
    <span class={cx(s.pill, s[tone], s[size], c)}>
      <I size={size === 'sm' ? 14 : 16} aria-hidden="true" class={tone === 'busy' ? s.spin : undefined} />
      <span>{children}</span>
    </span>
  );
}

export function serviceTone(state: ServiceState | null | undefined): StatusTone {
  switch (state) {
    case 'started':
      return 'ok';
    case 'starting':
    case 'stopping':
      return 'busy';
    case 'crashed':
      return 'danger';
    case 'stopped':
    case 'inactive':
      return 'neutral';
    default:
      return 'warn';
  }
}

export function ServiceStatus({ state, size }: { state: ServiceState | null | undefined; size?: 'sm' | 'md' }) {
  const st = state ?? 'unknown';
  const tone = serviceTone(st);
  return (
    <Status tone={tone} size={size} icon={st === 'stopped' || st === 'inactive' ? CircleStop : st === 'unknown' ? CircleExclamationPoint : undefined}>
      {t(`state.${st}`)}
    </Status>
  );
}

/** A small coloured dot with a text label beside it, for dense lists. */
export function Dot({ tone, label }: { tone: StatusTone; label: string }) {
  return (
    <span class={s.dotWrap}>
      <span class={cx(s.dot, s[tone])} aria-hidden="true" />
      <span>{label}</span>
    </span>
  );
}
