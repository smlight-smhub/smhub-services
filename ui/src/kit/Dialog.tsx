// Modal dialogs on the native <dialog> (D29): focus is trapped and Escape
// closes by the browser itself. On a phone a dialog rises as a bottom sheet.
import type { ComponentChildren } from 'preact';
import { useId, useLayoutEffect, useRef } from 'preact/hooks';
import { X } from 'lucide-preact';
import { t } from '../text/i18n';
import { IconButton } from './Button';
import { cx } from './cx';
import s from './Dialog.module.css';

interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: ComponentChildren;
  children?: ComponentChildren;
  footer?: ComponentChildren;
  size?: 'sm' | 'md' | 'lg' | 'full';
  /** Submit handler: the body becomes a form, so Enter submits. */
  onSubmit?: () => void;
  description?: ComponentChildren;
}

export function Dialog({ open, onClose, title, children, footer, size = 'md', onSubmit, description }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const tid = useId();
  const did = useId();
  useLayoutEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    else if (!open && d.open) d.close();
  }, [open]);

  const content = (
    <>
      <header class={s.head}>
        <div class={s.titles}>
          <h2 id={tid} class={s.title}>
            {title}
          </h2>
          {description && (
            <p id={did} class={s.desc}>
              {description}
            </p>
          )}
        </div>
        <IconButton icon={X} label={t('common.close')} onClick={onClose} />
      </header>
      <div class={s.body}>{children}</div>
      {footer && <footer class={s.foot}>{footer}</footer>}
    </>
  );

  return (
    <dialog
      ref={ref}
      class={cx(s.dialog, s[size])}
      aria-labelledby={tid}
      aria-describedby={description ? did : undefined}
      onClose={() => open && onClose()}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      {open &&
        (onSubmit ? (
          <form
            class={s.inner}
            onSubmit={(e) => {
              e.preventDefault();
              onSubmit();
            }}
          >
            {content}
          </form>
        ) : (
          <div class={s.inner}>{content}</div>
        ))}
    </dialog>
  );
}
