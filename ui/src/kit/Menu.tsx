// "More actions" for a row: a button that opens a short list of actions in a
// dialog (a sheet on phones). Keyboard and screen readers get a plain list of buttons.
import type { LucideIcon } from 'lucide-preact';
import { useState } from 'preact/hooks';
import { Ellipsis } from 'lucide-preact';
import { IconButton } from './Button';
import { Dialog } from './Dialog';
import { cx } from './cx';
import s from './Menu.module.css';

export interface MenuAction {
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  hidden?: boolean;
}

export function ActionMenu({ label, title, actions }: { label: string; title: string; actions: MenuAction[] }) {
  const [open, setOpen] = useState(false);
  const shown = actions.filter((a) => !a.hidden);
  return (
    <>
      <IconButton icon={Ellipsis} label={label} size="sm" onClick={() => setOpen(true)} aria-haspopup="dialog" />
      <Dialog open={open} onClose={() => setOpen(false)} title={title} size="sm">
        <ul class={s.list}>
          {shown.map((a) => {
            const I = a.icon;
            return (
              <li key={a.label}>
                <button
                  type="button"
                  class={cx(s.item, a.danger && s.danger)}
                  disabled={a.disabled}
                  onClick={() => {
                    setOpen(false);
                    a.onSelect();
                  }}
                >
                  {I && <I size={18} aria-hidden="true" />}
                  <span>{a.label}</span>
                </button>
              </li>
            );
          })}
        </ul>
      </Dialog>
    </>
  );
}
