// Tabs with the keyboard pattern of the ARIA guide: arrows move and select.
// The parent passes the same `id` to <Tabs> and <TabPanel> so they point at each other.
import type { ComponentChildren } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { useRef } from 'preact/hooks';
import { cx } from './cx';
import s from './Tabs.module.css';

export interface Tab<V extends string> {
  id: V;
  label: string;
  icon?: LucideIcon;
  badge?: ComponentChildren;
}

export function Tabs<V extends string>({ id, tabs, value, onChange, label }: { id: string; tabs: Tab<V>[]; value: V; onChange: (v: V) => void; label: string }) {
  const list = useRef<HTMLDivElement>(null);
  const move = (dir: number | 'first' | 'last') => {
    const i = tabs.findIndex((x) => x.id === value);
    const n = tabs.length;
    const j = dir === 'first' ? 0 : dir === 'last' ? n - 1 : (i + dir + n) % n;
    onChange(tabs[j]!.id);
    requestAnimationFrame(() => list.current?.querySelectorAll<HTMLButtonElement>('[role=tab]')[j]?.focus());
  };
  return (
    <div
      ref={list}
      class={s.tabs}
      role="tablist"
      aria-label={label}
      onKeyDown={(e) => {
        const rtl = document.dir === 'rtl';
        if (e.key === 'ArrowRight') move(rtl ? -1 : 1);
        else if (e.key === 'ArrowLeft') move(rtl ? 1 : -1);
        else if (e.key === 'Home') move('first');
        else if (e.key === 'End') move('last');
        else return;
        e.preventDefault();
      }}
    >
      {tabs.map((tab) => {
        const I = tab.icon;
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            id={`${id}-tab-${tab.id}`}
            aria-selected={selected}
            aria-controls={selected ? `${id}-panel` : undefined}
            tabIndex={selected ? 0 : -1}
            class={cx(s.tab, selected && s.selected)}
            onClick={() => onChange(tab.id)}
          >
            {I && <I size={16} aria-hidden="true" />}
            <span>{tab.label}</span>
            {tab.badge}
          </button>
        );
      })}
    </div>
  );
}

/** The panel of the selected tab. */
export function TabPanel({ id, tab, children }: { id: string; tab: string; children: ComponentChildren }) {
  return (
    <div role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-tab-${tab}`} class={s.panel} key={tab}>
      {children}
    </div>
  );
}
