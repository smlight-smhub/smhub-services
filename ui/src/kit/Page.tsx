// Page structure: a heading with optional back link and actions, sections of
// settings, rows and key/value lists. Opening a page moves focus to its
// heading, so screen readers announce where the user is.
import type { ComponentChildren } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { useEffect, useId, useRef } from 'preact/hooks';
import { ArrowLeft } from 'lucide-preact';
import { system } from '../state/system';
import { LinkButton } from './Button';
import { cx } from './cx';
import s from './Page.module.css';

let firstPage = true;

/** Sets the browser tab's title for a page: "Apps · smhub". */
export function useDocumentTitle(title: string) {
  const host = system.value?.hostname;
  useEffect(() => {
    document.title = host ? `${title} · ${host}` : title;
  }, [title, host]);
}

/** Moves focus to the page heading after in-app navigation (not on first load). */
export function useHeadingFocus(ref: { current: HTMLElement | null }) {
  useEffect(() => {
    if (firstPage) firstPage = false;
    else ref.current?.focus({ preventScroll: true });
  }, []);
}

export function Page({
  title,
  subtitle,
  back,
  actions,
  children,
  lead,
  narrow,
}: {
  title: string;
  subtitle?: ComponentChildren;
  back?: { href: string; label: string };
  actions?: ComponentChildren;
  children: ComponentChildren;
  /** Shown before the title, e.g. an app icon. */
  lead?: ComponentChildren;
  narrow?: boolean;
}) {
  const h1 = useRef<HTMLHeadingElement>(null);
  useDocumentTitle(title);
  useHeadingFocus(h1);
  return (
    <div class={cx(s.page, narrow && s.narrow)}>
      <header class={s.header}>
        {back && <LinkButton href={back.href} variant="ghost" size="sm" icon={ArrowLeft} class={s.back}>{back.label}</LinkButton>}
        <div class={s.titleRow}>
          {lead}
          <div class={s.titles}>
            <h1 ref={h1} tabIndex={-1} class={s.title}>
              {title}
            </h1>
            {subtitle && <div class={s.subtitle}>{subtitle}</div>}
          </div>
          {actions && <div class={s.actions}>{actions}</div>}
        </div>
      </header>
      <div class={s.content}>{children}</div>
    </div>
  );
}

/** A group of settings with a heading, drawn as a card. */
export function Section({
  title,
  description,
  icon: I,
  actions,
  children,
  id,
  footer,
}: {
  title?: ComponentChildren;
  description?: ComponentChildren;
  icon?: LucideIcon;
  actions?: ComponentChildren;
  children: ComponentChildren;
  id?: string;
  footer?: ComponentChildren;
}) {
  const hid = useId();
  return (
    <section class={s.section} aria-labelledby={title ? hid : undefined} id={id}>
      {(title || actions) && (
        <header class={s.sectionHead}>
          {I && (
            <span class={s.sectionIcon} aria-hidden="true">
              <I size={18} />
            </span>
          )}
          <div class={s.sectionTitles}>
            {title && (
              <h2 id={hid} class={s.sectionTitle}>
                {title}
              </h2>
            )}
            {description && <p class={s.sectionDesc}>{description}</p>}
          </div>
          {actions && <div class={s.actions}>{actions}</div>}
        </header>
      )}
      <div class={s.sectionBody}>{children}</div>
      {footer && <footer class={s.sectionFoot}>{footer}</footer>}
    </section>
  );
}

/** Rows separated by thin lines, inside a section. */
export function Rows({ children }: { children: ComponentChildren }) {
  return <div class={s.rows}>{children}</div>;
}

/** A label with a value or a control at the end. */
export function Row({ label, description, children, icon: I }: { label: ComponentChildren; description?: ComponentChildren; children?: ComponentChildren; icon?: LucideIcon }) {
  return (
    <div class={s.row}>
      {I && <I size={18} class={s.rowIcon} aria-hidden="true" />}
      <div class={s.rowText}>
        <div class={s.rowLabel}>{label}</div>
        {description && <div class={s.rowDesc}>{description}</div>}
      </div>
      {children !== undefined && <div class={s.rowValue}>{children}</div>}
    </div>
  );
}

/** A list of facts. Values that are missing are left out. */
export function Facts({ items, columns }: { items: [ComponentChildren, ComponentChildren | null | undefined | false][]; columns?: boolean }) {
  const shown = items.filter(([, v]) => v !== null && v !== undefined && v !== false && v !== '');
  return (
    <dl class={cx(s.facts, columns && s.factsColumns)}>
      {shown.map(([k, v], i) => (
        <div key={i} class={s.fact}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Buttons at the end of a form. */
export function Actions({ children, sticky }: { children: ComponentChildren; sticky?: boolean }) {
  // data-sticky-actions lifts the toasts above this bar so they never cover its buttons
  return (
    <div class={cx(s.formActions, sticky && s.sticky)} data-sticky-actions={sticky ? '' : undefined}>
      {children}
    </div>
  );
}

/** A vertical stack with the standard gap. */
export function Stack({ children, gap = 4, class: c }: { children: ComponentChildren; gap?: 2 | 3 | 4 | 5; class?: string }) {
  return <div class={cx(s.stack, s[`gap${gap}`], c)}>{children}</div>;
}
