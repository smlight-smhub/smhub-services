import type { ComponentChildren, JSX } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { useId } from 'preact/hooks';
import { cx } from './cx';
import s from './Card.module.css';

interface CardProps {
  title?: ComponentChildren;
  icon?: LucideIcon;
  /** Shown at the end of the header. */
  action?: ComponentChildren;
  /** Shown before the title (e.g. a drag handle). */
  lead?: ComponentChildren;
  children?: ComponentChildren;
  footer?: ComponentChildren;
  class?: string;
  id?: string;
  /** No inner padding (lists that run to the edge). */
  flush?: boolean;
  headingLevel?: 2 | 3;
  tone?: 'accent' | 'warn' | 'danger';
  attrs?: JSX.HTMLAttributes<HTMLElement>;
}

export function Card({ title, icon: I, action, lead, children, footer, class: c, id, flush, headingLevel = 2, tone, attrs }: CardProps) {
  const hid = useId();
  const H = headingLevel === 3 ? 'h3' : 'h2';
  return (
    <section id={id} class={cx(s.card, tone && s[tone], c)} aria-labelledby={title ? hid : undefined} {...attrs}>
      {(title || action || lead) && (
        <header class={s.head}>
          {lead}
          {I && (
            <span class={s.icon} aria-hidden="true">
              <I size={18} />
            </span>
          )}
          {title && (
            <H id={hid} class={s.title}>
              {title}
            </H>
          )}
          {action && <div class={s.action}>{action}</div>}
        </header>
      )}
      <div class={cx(s.body, flush && s.flush)}>{children}</div>
      {footer && <footer class={s.foot}>{footer}</footer>}
    </section>
  );
}
