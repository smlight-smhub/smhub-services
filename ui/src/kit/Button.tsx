import type { ButtonHTMLAttributes, ComponentChildren } from 'preact';
import { Play, type LucideIcon } from 'lucide-preact';
import { Link } from 'wouter-preact';
import { cx } from './cx';
import s from './Button.module.css';
import { t } from '../text/i18n';

export type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'soft';
type Size = 'sm' | 'md' | 'lg';

interface Common {
  variant?: Variant;
  size?: Size;
  icon?: LucideIcon;
  iconEnd?: LucideIcon;
  block?: boolean;
  class?: string;
  children?: ComponentChildren;
}

type ButtonProps = Common &
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'icon' | 'size' | 'class' | 'type' | 'disabled'> & {
    type?: 'button' | 'submit' | 'reset';
    busy?: boolean;
    disabled?: boolean;
  };

function Inner({ icon: I, iconEnd: E, busy, children, size }: Common & { busy?: boolean }) {
  const px = size === 'sm' ? 16 : 18;
  return (
    <>
      {busy ? <span class={s.spinner} aria-hidden="true" /> : I && <I size={px} aria-hidden="true" />}
      {children !== undefined && children !== null && children !== false && <span class={s.label}>{children}</span>}
      {E && <E size={px - 2} aria-hidden="true" />}
    </>
  );
}

export function Button({ variant = 'secondary', size = 'md', icon, iconEnd, busy, block, children, class: c, disabled, type = 'button', ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      class={cx(s.btn, s[variant], s[size], block && s.block, !children && s.iconOnly, c)}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      <Inner icon={icon} iconEnd={iconEnd} busy={busy} size={size}>
        {children}
      </Inner>
    </button>
  );
}

type IconButtonProps = Omit<ButtonProps, 'children' | 'icon'> & { icon: LucideIcon; label: string };

/** A button with only an icon; the label is its accessible name and tooltip. */
export function IconButton({ icon, label, variant = 'ghost', size = 'md', class: c, ...rest }: IconButtonProps) {
  return <Button {...rest} icon={icon} variant={variant} size={size} aria-label={label} title={label} class={cx(s.iconOnly, c)} />;
}

type LinkProps = Common & { href: string; external?: boolean; download?: boolean | string; label?: string; onClick?: (e: MouseEvent) => void };

/** A link that looks like a button. External links open in a new tab. */
export function LinkButton({ href, external, download, variant = 'secondary', size = 'md', icon, iconEnd, block, children, class: c, label, onClick }: LinkProps) {
  const cls = cx(s.btn, s[variant], s[size], block && s.block, !children && s.iconOnly, c);
  const inner = (
    <Inner icon={icon} iconEnd={iconEnd} size={size}>
      {children}
    </Inner>
  );
  if (external || download !== undefined) {
    return (
      <a
        class={cls}
        href={href}
        download={download === true ? '' : download || undefined}
        target={external ? '_blank' : undefined}
        rel={external ? 'noopener noreferrer' : undefined}
        aria-label={label}
        title={label}
        onClick={onClick}
      >
        {inner}
      </a>
    );
  }
  return (
    <Link href={href} class={cls} aria-label={label} title={label} onClick={onClick}>
      {inner}
    </Link>
  );
}

/**
 * Shown in place of Start/Stop while a service is starting or stopping: the hub cannot take
 * another command until it has finished, so there is nothing to click.
 */
export function Transition({ state, name, compact }: { state: 'starting' | 'stopping'; name: string; compact?: boolean }) {
  const label = state === 'starting' ? t('state.startingNamed', { name }) : t('state.stoppingNamed', { name });
  return compact ? (
    <IconButton size="sm" icon={Play} label={label} busy disabled />
  ) : (
    <Button busy disabled aria-label={label}>
      {state === 'starting' ? t('state.startingNow') : t('state.stoppingNow')}
    </Button>
  );
}

/** "starting" or "stopping" when a service is between states, else null. */
export function transitionOf(state: string | null | undefined): 'starting' | 'stopping' | null {
  return state === 'starting' || state === 'stopping' ? state : null;
}
