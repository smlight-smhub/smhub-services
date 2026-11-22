// Form controls on native elements (D29): the browser handles focus,
// keyboard and screen readers. Every control has a visible label, an
// optional hint and a place for the hub's message about that field.
import type { ComponentChildren, InputHTMLAttributes, TextareaHTMLAttributes } from 'preact';
import type { LucideIcon } from 'lucide-preact';
import { useId, useState } from 'preact/hooks';
import { CircleExclamationPoint, Eye, EyeOff, Plus, Trash } from 'lucide-preact';
import { t } from '../text/i18n';
import { cx } from './cx';
import s from './Field.module.css';

interface Base {
  label: ComponentChildren;
  hint?: ComponentChildren;
  error?: string;
  id?: string;
  class?: string;
  /** Hide the label visually (it stays for screen readers). */
  hideLabel?: boolean;
}

function ids(id?: string) {
  const auto = useId();
  const base = id ?? auto;
  return { id: base, hint: `${base}-hint`, error: `${base}-error` };
}

function described(i: ReturnType<typeof ids>, hint: unknown, error: unknown) {
  return [hint ? i.hint : '', error ? i.error : ''].filter(Boolean).join(' ') || undefined;
}

function Wrap({ i, label, hint, error, children, class: c, hideLabel, required }: Base & { i: ReturnType<typeof ids>; children: ComponentChildren; required?: boolean }) {
  return (
    <div class={cx(s.field, c)}>
      <label for={i.id} class={cx(s.label, hideLabel && 'visually-hidden')}>
        {label}
        {required && <span class={s.req} aria-hidden="true"> *</span>}
      </label>
      {children}
      {hint && (
        <p id={i.hint} class={s.hint}>
          {hint}
        </p>
      )}
      {error && <FieldError id={i.error} text={error} />}
    </div>
  );
}

export function FieldError({ id, text }: { id?: string; text: string }) {
  return (
    <p id={id} class={s.error} role="alert">
      <CircleExclamationPoint size={16} aria-hidden="true" />
      <span>{text}</span>
    </p>
  );
}

type InputAttrs = Omit<InputHTMLAttributes<HTMLInputElement>, 'label' | 'value' | 'onInput' | 'class' | 'type' | 'id' | 'icon'>;

export function TextField({
  label,
  hint,
  error,
  id,
  class: c,
  hideLabel,
  value,
  onValue,
  type = 'text',
  mono,
  trailing,
  icon: I,
  required,
  readOnly,
  ...rest
}: Base &
  InputAttrs & {
    value: string;
    onValue: (v: string) => void;
    type?: 'text' | 'password' | 'email' | 'search' | 'url' | 'tel' | 'time';
    mono?: boolean;
    trailing?: ComponentChildren;
    icon?: LucideIcon;
    required?: boolean;
    readOnly?: boolean;
  }) {
  const i = ids(id);
  const [shown, setShown] = useState(false);
  const isPassword = type === 'password';
  return (
    <Wrap i={i} label={label} hint={hint} error={error} class={c} hideLabel={hideLabel} required={required && !readOnly}>
      <div class={cx(s.control, readOnly && s.readOnly, error && s.invalid)}>
        {I && <I class={s.lead} size={18} aria-hidden="true" />}
        <input
          id={i.id}
          class={cx(s.input, mono && 'mono')}
          type={isPassword && shown ? 'text' : type}
          value={value}
          onInput={(e) => onValue((e.currentTarget as HTMLInputElement).value)}
          aria-invalid={error ? true : undefined}
          aria-describedby={described(i, hint, error)}
          required={required && !readOnly}
          readOnly={readOnly}
          spellcheck={isPassword || mono ? false : undefined}
          autoCapitalize={isPassword || mono ? 'off' : undefined}
          {...rest}
        />
        {isPassword && (
          <button
            type="button"
            class={s.reveal}
            onClick={() => setShown(!shown)}
            aria-label={shown ? t('field.hidePassword') : t('field.showPassword')}
            aria-pressed={shown}
            title={shown ? t('field.hidePassword') : t('field.showPassword')}
          >
            {shown ? <EyeOff size={18} aria-hidden="true" /> : <Eye size={18} aria-hidden="true" />}
          </button>
        )}
        {trailing}
      </div>
    </Wrap>
  );
}

export function NumberField({
  label,
  hint,
  error,
  id,
  class: c,
  value,
  onValue,
  min,
  max,
  step,
  required,
  unit,
  readOnly,
  ...rest
}: Base &
  Omit<InputAttrs, 'min' | 'max' | 'step'> & {
    value: number | null | undefined;
    onValue: (v: number | null) => void;
    min?: number;
    max?: number;
    step?: number | 'any';
    required?: boolean;
    unit?: string;
    readOnly?: boolean;
  }) {
  const i = ids(id);
  return (
    <Wrap i={i} label={label} hint={hint} error={error} class={c} required={required && !readOnly}>
      <div class={cx(s.control, s.narrow, readOnly && s.readOnly, error && s.invalid)}>
        <input
          id={i.id}
          class={cx(s.input, 'num')}
          type="number"
          inputMode={step === 'any' ? 'decimal' : 'numeric'}
          // a string, as the DOM keeps it: Preact compares strictly, and a number would make
          // every re-render (live data) rewrite the field and lose the caret and what was typed
          value={value == null ? '' : String(value)}
          min={min}
          max={max}
          step={step}
          required={required && !readOnly}
          readOnly={readOnly}
          onInput={(e) => {
            const v = (e.currentTarget as HTMLInputElement).value;
            onValue(v === '' ? null : Number(v));
          }}
          aria-invalid={error ? true : undefined}
          aria-describedby={described(i, hint, error)}
          {...rest}
        />
        {unit && (
          <span class={s.unit} aria-hidden="true">
            {unit}
          </span>
        )}
      </div>
    </Wrap>
  );
}

export function TextArea({
  label,
  hint,
  error,
  id,
  class: c,
  value,
  onValue,
  rows = 4,
  mono,
  required,
  ...rest
}: Base &
  Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'label' | 'value' | 'onInput' | 'class' | 'id' | 'rows'> & {
    value: string;
    onValue: (v: string) => void;
    rows?: number;
    mono?: boolean;
    required?: boolean;
  }) {
  const i = ids(id);
  return (
    <Wrap i={i} label={label} hint={hint} error={error} class={c} required={required}>
      <textarea
        id={i.id}
        class={cx(s.textarea, mono && 'mono', error && s.invalid)}
        value={value}
        rows={rows}
        required={required}
        spellcheck={mono ? false : undefined}
        onInput={(e) => onValue((e.currentTarget as HTMLTextAreaElement).value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={described(i, hint, error)}
        {...rest}
      />
    </Wrap>
  );
}

export interface Option<V extends string = string> {
  value: V;
  label: string;
  disabled?: boolean;
  hint?: string;
  icon?: LucideIcon;
}

export function SelectField<V extends string>({
  label,
  hint,
  error,
  id,
  class: c,
  hideLabel,
  value,
  onValue,
  options,
  disabled,
  required,
}: Base & { value: V; onValue: (v: V) => void; options: Option<V>[]; disabled?: boolean; required?: boolean }) {
  const i = ids(id);
  return (
    <Wrap i={i} label={label} hint={hint} error={error} class={c} hideLabel={hideLabel} required={required}>
      <div class={cx(s.control, s.select, error && s.invalid)}>
        <select
          id={i.id}
          class={s.input}
          value={value}
          disabled={disabled}
          required={required}
          onChange={(e) => onValue((e.currentTarget as HTMLSelectElement).value as V)}
          aria-invalid={error ? true : undefined}
          aria-describedby={described(i, hint, error)}
        >
          {options.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </select>
      </div>
    </Wrap>
  );
}

/** A settings row: label and description with a switch at the end. Saves on change. */
export function Switch({
  label,
  description,
  checked,
  onChange,
  disabled,
  busy,
  error,
  id,
}: {
  label: ComponentChildren;
  description?: ComponentChildren;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  busy?: boolean;
  error?: string;
  id?: string;
}) {
  const i = ids(id);
  return (
    <div class={s.switchRow}>
      <label class={s.switchLabel} for={i.id}>
        <span class={s.switchText}>
          <span class={s.switchTitle}>{label}</span>
          {description && (
            <span class={s.switchDesc} id={i.hint}>
              {description}
            </span>
          )}
        </span>
        <input
          id={i.id}
          type="checkbox"
          role="switch"
          class={cx(s.switch, busy && s.busy)}
          checked={checked}
          disabled={disabled}
          aria-describedby={described(i, description, error)}
          aria-busy={busy || undefined}
          onChange={(e) => onChange((e.currentTarget as HTMLInputElement).checked)}
        />
      </label>
      {error && <FieldError id={i.error} text={error} />}
    </div>
  );
}

export function Checkbox({ label, checked, onChange, disabled, hint }: { label: ComponentChildren; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; hint?: ComponentChildren }) {
  const i = ids();
  return (
    <div class={s.check}>
      <input
        id={i.id}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-describedby={hint ? i.hint : undefined}
        onChange={(e) => onChange((e.currentTarget as HTMLInputElement).checked)}
      />
      <label for={i.id}>
        {label}
        {hint && (
          <span id={i.hint} class={s.checkHint}>
            {hint}
          </span>
        )}
      </label>
    </div>
  );
}

/** One choice of a few: radio buttons drawn as a segmented control or as cards. */
export function Choice<V extends string>({
  label,
  hint,
  value,
  onValue,
  options,
  variant = 'segmented',
  error,
  hideLabel,
  disabled,
}: {
  label: ComponentChildren;
  hint?: ComponentChildren;
  value: V | undefined;
  onValue: (v: V) => void;
  options: Option<V>[];
  variant?: 'segmented' | 'cards';
  error?: string;
  hideLabel?: boolean;
  disabled?: boolean;
}) {
  const name = useId();
  const hintId = `${name}-hint`;
  return (
    <fieldset class={s.fieldset} aria-describedby={hint ? hintId : undefined} disabled={disabled}>
      <legend class={cx(s.label, hideLabel && 'visually-hidden')}>{label}</legend>
      <div class={variant === 'cards' ? s.cards : s.segmented}>
        {options.map((o) => {
          const I = o.icon;
          return (
            <label key={o.value} class={cx(s.option, value === o.value && s.selected, o.disabled && s.disabledOption)}>
              <input type="radio" name={name} value={o.value} checked={value === o.value} disabled={o.disabled} onChange={() => onValue(o.value)} />
              {I && <I size={variant === 'cards' ? 22 : 16} aria-hidden="true" />}
              <span class={s.optionText}>
                <span>{o.label}</span>
                {o.hint && variant === 'cards' && <span class={s.optionHint}>{o.hint}</span>}
              </span>
            </label>
          );
        })}
      </div>
      {hint && (
        <p id={hintId} class={s.hint}>
          {hint}
        </p>
      )}
      {error && <FieldError text={error} />}
    </fieldset>
  );
}

/** A list of short texts (addresses, DNS servers, topics): one row each. */
export function ListField({
  label,
  hint,
  error,
  value,
  onValue,
  placeholder,
  mono = true,
  max,
  addLabel,
}: Base & { value: string[]; onValue: (v: string[]) => void; placeholder?: string; mono?: boolean; max?: number; addLabel?: string }) {
  const name = useId();
  const items = value.length ? value : [''];
  const set = (i: number, v: string) => onValue(items.map((x, j) => (j === i ? v : x)));
  return (
    <fieldset class={s.fieldset} aria-describedby={hint ? `${name}-hint` : undefined}>
      <legend class={s.label}>{label}</legend>
      <ul class={s.list}>
        {items.map((v, i) => (
          <li key={i} class={cx(s.control, error && s.invalid)}>
            <input
              class={cx(s.input, mono && 'mono')}
              value={v}
              placeholder={placeholder}
              aria-label={`${typeof label === 'string' ? label : ''} ${i + 1}`}
              spellcheck={false}
              autoCapitalize="off"
              onInput={(e) => set(i, (e.currentTarget as HTMLInputElement).value)}
            />
            <button
              type="button"
              class={s.reveal}
              aria-label={t('field.removeItem', { n: String(i + 1) })}
              title={t('field.remove')}
              onClick={() => onValue(items.filter((_, j) => j !== i))}
              disabled={items.length === 1 && !v}
            >
              <Trash size={16} aria-hidden="true" />
            </button>
          </li>
        ))}
      </ul>
      {(max === undefined || items.length < max) && (
        <button type="button" class={s.add} onClick={() => onValue([...items, ''])}>
          <Plus size={16} aria-hidden="true" />
          {addLabel ?? t('field.add')}
        </button>
      )}
      {hint && (
        <p id={`${name}-hint`} class={s.hint}>
          {hint}
        </p>
      )}
      {error && <FieldError text={error} />}
    </fieldset>
  );
}

/** Lays fields out in a responsive grid. */
export function FieldGrid({ children, cols = 2 }: { children: ComponentChildren; cols?: 1 | 2 | 3 }) {
  return <div class={cx(s.grid, cols === 3 && s.grid3, cols === 1 && s.grid1)}>{children}</div>;
}
