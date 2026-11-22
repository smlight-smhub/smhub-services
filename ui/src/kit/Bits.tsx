// Small shared pieces: app icons, signal bars, copy buttons, keyboard keys.
import { useState } from 'preact/hooks';
import { Check, Copy } from 'lucide-preact';
import { t } from '../text/i18n';
import { percent } from '../text/format';
import { toast } from '../state/toasts';
import { Button, IconButton } from './Button';
import { cx } from './cx';
import s from './Bits.module.css';

export { appTitle } from '../text/names';

const HUES = [152, 200, 260, 20, 330, 45, 180, 100];

/** The app's icon from its package, or its initial on a coloured tile. */
export function AppIcon({ name, src, size = 40 }: { name: string; src?: string | null; size?: number }) {
  const [failed, setFailed] = useState(false);
  const style = { inlineSize: `${size}px`, blockSize: `${size}px`, borderRadius: `${Math.round(size * 0.28)}px` };
  if (src && !failed) {
    return <img class={s.icon} src={src} alt="" width={size} height={size} loading="lazy" decoding="async" style={style} onError={() => setFailed(true)} />;
  }
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = HUES[h % HUES.length];
  return (
    <span class={s.letter} aria-hidden="true" style={{ ...style, fontSize: `${Math.round(size * 0.42)}px`, '--hue': String(hue) }}>
      {(name[0] ?? '?').toUpperCase()}
    </span>
  );
}

/** Signal strength as four bars plus text for screen readers. */
export function SignalBars({ value, label }: { value: number | null | undefined; label?: string }) {
  const v = value ?? 0;
  const bars = v >= 75 ? 4 : v >= 50 ? 3 : v >= 25 ? 2 : v > 0 ? 1 : 0;
  const words = bars >= 4 ? t('signal.excellent') : bars === 3 ? t('signal.good') : bars === 2 ? t('signal.fair') : t('signal.weak');
  const text = label ?? t('signal.label', { quality: words, value: percent(v) });
  return (
    <span class={s.bars} role="img" aria-label={text} title={text}>
      {[1, 2, 3, 4].map((i) => (
        <span key={i} class={cx(s.bar, i <= bars && s.on, bars <= 1 && i <= bars && s.low)} style={{ blockSize: `${4 + i * 3}px` }} />
      ))}
    </span>
  );
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* fall back below */
  }
  // Hubs are often opened over plain HTTP, where the clipboard API is missing.
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.setAttribute('readonly', '');
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand('copy');
  } catch {
    ok = false;
  }
  ta.remove();
  return ok;
}

export function CopyButton({ text, label, compact }: { text: string; label?: string; compact?: boolean }) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    const ok = await writeClipboard(text);
    if (ok) {
      setDone(true);
      setTimeout(() => setDone(false), 1500);
      toast({ text: t('common.copied'), tone: 'ok', timeout: 2000 });
    } else toast({ text: t('common.copyFailed'), tone: 'warn' });
  };
  const l = label ?? t('common.copy');
  return compact ? (
    <IconButton icon={done ? Check : Copy} label={l} size="sm" onClick={copy} />
  ) : (
    <Button icon={done ? Check : Copy} size="sm" onClick={copy}>
      {l}
    </Button>
  );
}

/** A value in monospace with a copy button. */
export function CopyValue({ value, label }: { value: string; label: string }) {
  return (
    <span class={s.copyValue}>
      <code class={s.code}>{value}</code>
      <CopyButton text={value} label={label} compact />
    </span>
  );
}

export function Kbd({ children }: { children: string }) {
  return <kbd class={s.kbd}>{children}</kbd>;
}

/** Whether this is a Mac, for showing ⌘ instead of Ctrl. */
export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
