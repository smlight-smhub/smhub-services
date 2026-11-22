// Choosing one value from a long list (time zones, regions, currencies): a
// button that opens a dialog with a search field and the matching options.
import type { ComponentChildren } from 'preact';
import { useId, useMemo, useState } from 'preact/hooks';
import { Check, ChevronDown, Search } from 'lucide-preact';
import { t } from '../text/i18n';
import { Dialog } from './Dialog';
import { TextField } from './Field';
import { cx } from './cx';
import s from './SearchSelect.module.css';

export interface SearchOption {
  value: string;
  label: string;
  /** Extra text that the search also matches, e.g. a code. */
  detail?: string;
}

const SHOWN = 150;

export function SearchSelect({
  label,
  value,
  options,
  onValue,
  hint,
  error,
  placeholder,
  disabled,
}: {
  label: string;
  value: string;
  options: SearchOption[];
  onValue: (v: string) => void;
  hint?: ComponentChildren;
  error?: string;
  placeholder?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');
  const id = useId();
  const current = options.find((o) => o.value === value);
  const matches = useMemo(() => {
    const needle = q.trim().toLocaleLowerCase().replace(/[\s_/]+/g, ' ');
    if (!needle) return options;
    return options.filter((o) => `${o.label} ${o.detail ?? ''} ${o.value}`.toLocaleLowerCase().replace(/[\s_/]+/g, ' ').includes(needle));
  }, [q, options]);

  const pick = (v: string) => {
    onValue(v);
    setOpen(false);
    setQ('');
  };

  return (
    <div class={s.wrap}>
      <span class={s.label} id={`${id}-l`}>
        {label}
      </span>
      <button
        type="button"
        class={cx(s.button, error && s.invalid)}
        onClick={() => setOpen(true)}
        disabled={disabled}
        aria-haspopup="dialog"
        aria-labelledby={`${id}-l ${id}-v`}
        aria-describedby={hint ? `${id}-h` : undefined}
      >
        <span class={s.value} id={`${id}-v`}>
          {current ? current.label : value || placeholder || t('common.choose')}
          {current?.detail && <span class={s.detail}>{current.detail}</span>}
        </span>
        <ChevronDown size={18} aria-hidden="true" />
      </button>
      {hint && (
        <p class={s.hint} id={`${id}-h`}>
          {hint}
        </p>
      )}
      {error && (
        <p class={s.error} role="alert">
          {error}
        </p>
      )}
      <Dialog open={open} onClose={() => setOpen(false)} title={label} size="md">
        <TextField
          label={t('common.search')}
          hideLabel
          type="search"
          icon={Search}
          value={q}
          onValue={setQ}
          placeholder={t('common.searchPlaceholder')}
          autoFocus
          onKeyDown={(e) => {
            if (e.key === 'Enter' && matches[0]) {
              e.preventDefault();
              pick(matches[0].value);
            }
          }}
        />
        <p class="visually-hidden" role="status">
          {t('common.matches', { count: matches.length })}
        </p>
        <ul class={s.list}>
          {matches.slice(0, SHOWN).map((o) => (
            <li key={o.value}>
              <button type="button" class={cx(s.option, o.value === value && s.selected)} aria-current={o.value === value ? 'true' : undefined} onClick={() => pick(o.value)}>
                <span class={s.optionText}>
                  <span>{o.label}</span>
                  {o.detail && <span class={s.detail}>{o.detail}</span>}
                </span>
                {o.value === value && <Check size={18} aria-hidden="true" />}
              </button>
            </li>
          ))}
        </ul>
        {matches.length > SHOWN && <p class={s.more}>{t('common.moreMatches', { count: matches.length - SHOWN })}</p>}
        {!matches.length && <p class={s.more}>{t('common.noMatches')}</p>}
      </Dialog>
    </div>
  );
}
