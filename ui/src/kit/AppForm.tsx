// The one renderer for app settings forms (D33). The form comes from the
// package's schema.json through the API; the hub validates everything, the
// form only adds the hints the schema gives.
import { useEffect, useMemo, useState } from 'preact/hooks';
import { ChevronDown, ChevronUp, RotateCcw, Save } from 'lucide-preact';
import type { AppSettingValues, FormField } from '../api/types';
import { t } from '../text/i18n';
import { keyTitle } from '../text/names';
import { advanced } from '../state/prefs';
import { Button } from './Button';
import { Choice, ListField, NumberField, SelectField, Switch, TextField, type Option } from './Field';
import { Actions } from './Page';
import s from './AppForm.module.css';

/** Text from a package: its label, or a readable form of a key such as "serial_port". */
export function packageText(label: string | undefined, key: string): string {
  const raw = label || key;
  // A bare key ("serial_port", "baudRate", "zigbee.channel") becomes words;
  // any other text is shown exactly as the package wrote it.
  const isKey = /^[A-Za-z][A-Za-z0-9]*([_.-][A-Za-z0-9]+)*$/.test(raw) && (/[_.-]/.test(raw) || /^[a-z]/.test(raw));
  return isKey ? keyTitle(raw) : raw;
}

const NONE: string[] = [];

interface Group {
  sep?: FormField;
  fields: FormField[];
}

function groups(form: FormField[]): Group[] {
  const out: Group[] = [{ fields: [] }];
  for (const f of form) {
    if (f.type === 'separator') out.push({ sep: f, fields: [] });
    else out[out.length - 1]!.fields.push(f);
  }
  return out.filter((g) => g.sep || g.fields.length);
}

function initial(form: FormField[], values: AppSettingValues, secrets: string[]): AppSettingValues {
  const v: AppSettingValues = { ...values };
  for (const f of form) {
    // A saved password is never sent to the browser: the field starts empty.
    if (f.type === 'password' && secrets.includes(f.key)) {
      v[f.key] = '';
      continue;
    }
    if (f.type !== 'separator' && v[f.key] === undefined && f.default !== undefined) v[f.key] = f.default;
    if (f.toggle?.field && v[f.toggle.field] === undefined) v[f.toggle.field] = f.toggle.default ?? false;
  }
  return v;
}

function validate(f: FormField, v: unknown, secretSet: boolean): string | undefined {
  const empty = v === undefined || v === null || v === '' || (Array.isArray(v) && v.every((x) => x === ''));
  if (f.required && empty && !secretSet) return t('form.required');
  if (empty) return undefined;
  if ((f.type === 'number' || f.type === 'integer') && typeof v === 'number') {
    if (f.min !== undefined && v < f.min) return t('form.min', { min: f.min });
    if (f.max !== undefined && v > f.max) return t('form.max', { max: f.max });
    if (f.type === 'integer' && !Number.isInteger(v)) return t('form.integer');
  }
  if (f.pattern && typeof v === 'string') {
    try {
      if (!new RegExp(`^(?:${f.pattern})$`).test(v)) return t('form.pattern');
    } catch {
      /* a pattern the browser cannot read: the hub checks it */
    }
  }
  return undefined;
}

export function AppForm({
  form,
  values,
  secrets = NONE,
  onSave,
  errors: hubErrors,
}: {
  form: FormField[];
  values: AppSettingValues;
  /** Password fields that have a saved value (never sent to the browser). */
  secrets?: string[];
  /** Sends the changed values; answers true when saved. */
  onSave: (changed: AppSettingValues) => Promise<boolean>;
  errors?: Record<string, string>;
}) {
  const start = useMemo(() => initial(form, values, secrets), [form, values, secrets]);
  const [draft, setDraft] = useState<AppSettingValues>(start);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [local, setLocal] = useState<Record<string, string>>({});
  const [showMore, setShowMore] = useState(false);
  useEffect(() => {
    if (!dirty) setDraft(start);
  }, [start, dirty]);

  const showAdvanced = advanced.value || showMore;
  const hiddenCount = form.filter((f) => f.advanced && f.type !== 'separator').length;
  const set = (key: string, v: unknown) => {
    setDraft((d) => ({ ...d, [key]: v }));
    setDirty(true);
    setLocal((e) => {
      if (!e[key]) return e;
      const { [key]: _drop, ...rest } = e;
      return rest;
    });
  };
  const err = (key: string) => local[key] ?? hubErrors?.[key];

  // Settings the app owns (Z-Wave network keys among them): shown, never sent.
  const locked = useMemo(() => {
    const keys = new Set<string>();
    for (const f of form) {
      if (!f.read_only) continue;
      keys.add(f.key);
      if (f.toggle?.field) keys.add(f.toggle.field);
    }
    return keys;
  }, [form]);

  const submit = async (e: Event) => {
    e.preventDefault();
    const found: Record<string, string> = {};
    for (const g of groups(form)) {
      const on = !g.sep?.toggle?.field || !!draft[g.sep.toggle.field];
      if (!on) continue;
      for (const f of g.fields) {
        if (f.advanced && !showAdvanced) continue;
        if (f.read_only) continue;
        const m = validate(f, draft[f.key], secrets.includes(f.key));
        if (m) found[f.key] = m;
      }
    }
    setLocal(found);
    if (Object.keys(found).length) {
      document.getElementById(`af-${Object.keys(found)[0]}`)?.focus();
      return;
    }
    const changed: AppSettingValues = {};
    for (const k of Object.keys(draft)) {
      if (locked.has(k)) continue;
      // An empty password field keeps the saved password: it is left out.
      if (secrets.includes(k) && draft[k] === '') continue;
      if (JSON.stringify(draft[k]) !== JSON.stringify(start[k])) changed[k] = draft[k];
    }
    if (!Object.keys(changed).length) {
      setDirty(false);
      return;
    }
    setBusy(true);
    const ok = await onSave(changed);
    setBusy(false);
    if (ok) setDirty(false);
  };

  const field = (f: FormField) => {
    if (f.advanced && !showAdvanced) return null;
    const label = packageText(f.label, f.key);
    const ro = !!f.read_only;
    const own = f.description ? packageText(f.description, f.description) : undefined;
    const hint = ro ? (own ? `${own} ${t('form.readOnly')}` : t('form.readOnly')) : own;
    const v = draft[f.key];
    const id = `af-${f.key}`;
    if (ro && f.type === 'list') {
      const text = Array.isArray(v) ? v.map(String).join(', ') : '';
      return <TextField key={f.key} id={id} label={label} hint={hint} value={text} onValue={() => undefined} readOnly mono />;
    }
    switch (f.type) {
      case 'boolean':
        return <Switch key={f.key} id={id} label={label} description={hint} checked={!!v} onChange={(x) => set(f.key, x)} disabled={ro} error={err(f.key)} />;
      case 'number':
      case 'integer':
        return (
          <NumberField
            key={f.key}
            id={id}
            label={label}
            hint={hint}
            value={typeof v === 'number' ? v : v === undefined || v === null || v === '' ? null : Number(v)}
            onValue={(x) => set(f.key, x === null ? null : f.type === 'integer' ? Math.round(x) : x)}
            min={f.min}
            max={f.max}
            step={f.type === 'integer' ? 1 : 'any'}
            required={f.required}
            readOnly={ro}
            error={err(f.key)}
          />
        );
      case 'enum': {
        const opts = f.options ?? [];
        const options: Option[] = opts.map((o, i) => ({ value: String(i), label: o.label ? packageText(o.label, String(o.value)) : String(o.value) }));
        const idx = opts.findIndex((o) => o.value === v || String(o.value) === String(v));
        const onValue = (x: string) => set(f.key, opts[Number(x)]?.value);
        return options.length <= 3 && options.every((o) => o.label.length < 18) ? (
          <Choice key={f.key} label={label} hint={hint} value={idx >= 0 ? String(idx) : undefined} onValue={onValue} options={options} disabled={ro} error={err(f.key)} />
        ) : (
          <SelectField key={f.key} id={id} label={label} hint={hint} value={idx >= 0 ? String(idx) : ''} onValue={onValue} options={idx >= 0 ? options : [{ value: '', label: t('common.choose'), disabled: true }, ...options]} required={f.required && !ro} disabled={ro} error={err(f.key)} />
        );
      }
      case 'list':
        return <ListField key={f.key} label={label} hint={hint} value={Array.isArray(v) ? v.map(String) : []} onValue={(x) => set(f.key, x)} error={err(f.key)} />;
      case 'password':
      case 'text':
      default:
        return (
          <TextField
            key={f.key}
            id={id}
            label={label}
            hint={hint}
            type={f.type === 'password' ? 'password' : 'text'}
            value={v === undefined || v === null ? '' : String(v)}
            onValue={(x) => set(f.key, x)}
            required={f.required}
            readOnly={ro}
            mono={ro || undefined}
            pattern={f.pattern}
            placeholder={f.type === 'password' && secrets.includes(f.key) ? t('form.secretSet') : undefined}
            autoComplete={f.type === 'password' ? 'new-password' : 'off'}
            error={err(f.key)}
          />
        );
    }
  };

  return (
    <form class={s.form} onSubmit={submit} noValidate>
      {groups(form).map((g, gi) => {
        const toggle = g.sep?.toggle?.field;
        const on = !toggle || !!draft[toggle];
        const fields = on ? g.fields.map(field).filter(Boolean) : [];
        if (g.sep?.advanced && !showAdvanced) return null;
        if (!g.sep && !fields.length) return null;
        return (
          <fieldset key={gi} class={s.group}>
            {g.sep && <legend class={s.legend}>{packageText(g.sep.label, g.sep.key)}</legend>}
            {g.sep?.description && <p class={s.groupDesc}>{packageText(g.sep.description, g.sep.description)}</p>}
            {toggle && (
              <Switch
                label={t('form.enableGroup', { name: packageText(g.sep?.label, g.sep?.key ?? toggle) })}
                description={g.sep?.read_only ? t('form.readOnly') : undefined}
                checked={on}
                onChange={(x) => set(toggle, x)}
                disabled={!!g.sep?.read_only}
              />
            )}
            {fields}
          </fieldset>
        );
      })}
      {!advanced.value && hiddenCount > 0 && (
        <Button variant="ghost" size="sm" icon={showMore ? ChevronUp : ChevronDown} onClick={() => setShowMore(!showMore)} aria-expanded={showMore}>
          {showMore ? t('form.fewer') : t('form.more', { count: hiddenCount })}
        </Button>
      )}
      {dirty && (
        <Actions sticky>
          <Button
            variant="ghost"
            icon={RotateCcw}
            onClick={() => {
              setDraft(start);
              setDirty(false);
              setLocal({});
            }}
          >
            {t('common.discard')}
          </Button>
          <Button type="submit" variant="primary" icon={Save} busy={busy}>
            {t('common.save')}
          </Button>
        </Actions>
      )}
    </form>
  );
}
