import type { ComponentChildren } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { CircleCheck, CircleX, Info, RefreshCw, TriangleExclamationPoint } from 'lucide-preact';
import { t } from '../text/i18n';
import { percent } from '../text/format';
import { errorText } from '../state/save';
import { Button } from './Button';
import { cx } from './cx';
import s from './Feedback.module.css';

type Tone = 'ok' | 'info' | 'warn' | 'danger';
const ICON: Record<Tone, LucideIcon> = { ok: CircleCheck, info: Info, warn: TriangleExclamationPoint, danger: CircleX };

/** A message inside a page: colour, icon and text, with an optional action. */
export function Banner({
  tone = 'info',
  title,
  children,
  action,
  icon,
  class: c,
  live,
}: {
  tone?: Tone;
  title?: ComponentChildren;
  children?: ComponentChildren;
  action?: ComponentChildren;
  icon?: LucideIcon;
  class?: string;
  /** Announce it to screen readers when it appears. */
  live?: boolean;
}) {
  const I = icon ?? ICON[tone];
  return (
    <div class={cx(s.banner, s[tone], c)} role={live ? (tone === 'danger' ? 'alert' : 'status') : undefined}>
      <I size={20} class={s.bannerIcon} aria-hidden="true" />
      <div class={s.bannerText}>
        {title && <p class={s.bannerTitle}>{title}</p>}
        {children && <div class={s.bannerBody}>{children}</div>}
      </div>
      {action && <div class={s.bannerAction}>{action}</div>}
    </div>
  );
}

export function EmptyState({ icon: I, title, children, action }: { icon?: LucideIcon; title: ComponentChildren; children?: ComponentChildren; action?: ComponentChildren }) {
  return (
    <div class={s.empty}>
      {I && (
        <span class={s.emptyIcon} aria-hidden="true">
          <I size={28} />
        </span>
      )}
      <p class={s.emptyTitle}>{title}</p>
      {children && <div class={s.emptyText}>{children}</div>}
      {action && <div class={s.emptyAction}>{action}</div>}
    </div>
  );
}

/** A failed read, with a way to try again. */
export function ErrorState({ error, retry }: { error: unknown; retry?: () => void }) {
  return (
    <Banner
      tone="danger"
      title={t('error.loadFailed')}
      action={
        retry && (
          <Button size="sm" icon={RefreshCw} onClick={retry}>
            {t('common.retry')}
          </Button>
        )
      }
    >
      {errorText(error)}
    </Banner>
  );
}

/** A placeholder for data that is truly unknown (never for known data, D38). */
export function Skeleton({ lines = 1, height, width, class: c }: { lines?: number; height?: number; width?: string; class?: string }) {
  return (
    <div class={cx(s.skeletonGroup, c)} aria-hidden="true">
      {Array.from({ length: lines }, (_, i) => (
        <span
          key={i}
          class={s.skeleton}
          style={{ blockSize: height ? `${height}px` : undefined, inlineSize: width ?? (lines > 1 && i === lines - 1 ? '60%' : '100%') }}
        />
      ))}
    </div>
  );
}

/** Loading text for screen readers with a quiet skeleton for the eyes. */
export function Loading({ lines = 3 }: { lines?: number }) {
  return (
    <div role="status" aria-live="polite">
      <span class="visually-hidden">{t('common.loading')}</span>
      <Skeleton lines={lines} />
    </div>
  );
}

/** Progress of a task: value 0–100, or null when unknown. */
export function ProgressBar({ value, label, tone = 'accent' }: { value: number | null | undefined; label: string; tone?: 'accent' | 'danger' | 'ok' }) {
  const known = value !== null && value !== undefined;
  return (
    <div
      class={cx(s.progress, !known && s.indeterminate, s[tone])}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={known ? Math.round(value) : undefined}
      aria-valuetext={known ? percent(value) : t('common.working')}
    >
      <span class={s.progressFill} style={{ inlineSize: known ? `${Math.max(0, Math.min(100, value))}%` : undefined }} />
    </div>
  );
}

/** How full something is (storage, memory): 0–100, coloured by thresholds. */
export function Meter({ value, label, valueText, warnAt = 80, dangerAt = 92 }: { value: number; label: string; valueText?: string; warnAt?: number; dangerAt?: number }) {
  const tone = value >= dangerAt ? 'danger' : value >= warnAt ? 'warn' : 'ok';
  return (
    <div
      class={cx(s.meter, s[`m_${tone}`])}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      aria-valuetext={valueText ?? percent(value)}
    >
      <span class={s.meterFill} style={{ inlineSize: `${Math.max(1, Math.min(100, value))}%` }} />
    </div>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <span class={s.spinnerWrap} role={label ? 'status' : undefined}>
      <span class={s.spinner} aria-hidden="true" />
      {label && <span class="visually-hidden">{label}</span>}
    </span>
  );
}
