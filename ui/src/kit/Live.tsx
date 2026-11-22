// Live values: a gentle number tween and a sparkline in inline SVG (no chart library).
import { useEffect, useRef, useState } from 'preact/hooks';
import { reducedMotion } from './cx';
import s from './Live.module.css';

/** Moves smoothly to a new value over ~450 ms; instant with reduced motion or a hidden tab. */
export function useTween(target: number, ms = 450): number {
  const [v, setV] = useState(target);
  const current = useRef(target);
  useEffect(() => {
    if (!Number.isFinite(target)) {
      current.current = target;
      setV(target);
      return;
    }
    const from = current.current;
    if (!Number.isFinite(from) || from === target || reducedMotion() || document.hidden) {
      current.current = target;
      setV(target);
      return;
    }
    const start = performance.now();
    let raf = 0;
    const step = (now: number) => {
      const p = Math.min(1, (now - start) / ms);
      const eased = 1 - Math.pow(1 - p, 3);
      const x = from + (target - from) * eased;
      current.current = x;
      setV(x);
      if (p < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, ms]);
  return v;
}

/** A number that tweens to each new value; `format` turns it into text. */
export function Tween({ value, format, class: c }: { value: number; format: (v: number) => string; class?: string }) {
  const v = useTween(value);
  return <span class={c ? `num ${c}` : 'num'}>{format(v)}</span>;
}

/**
 * A small line chart of recent values (0–max). Decorative: the current value
 * is always shown as text next to it.
 */
export function Sparkline({ values, max = 100, tone = 1, label }: { values: number[]; max?: number; tone?: 1 | 2; label?: string }) {
  const W = 120;
  const H = 36;
  if (!values.length) return <div class={s.spark} aria-hidden="true" />;
  // Until there is enough history the line starts flat at the first value,
  // so the chart spans its width from the first sample.
  const SPAN = 30;
  if (values.length < SPAN) values = [...Array<number>(SPAN - values.length).fill(values[0]!), ...values];
  const n = values.length;
  const step = W / (n - 1);
  const x0 = 0;
  const pts = values.map((v, i) => [x0 + i * step, H - 2 - (Math.max(0, Math.min(max, v)) / max) * (H - 4)] as const);
  const line = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join('');
  const area = `${line}L${W} ${H}L${x0.toFixed(1)} ${H}Z`;
  const last = pts[n - 1]!;
  return (
    <svg class={`${s.spark} ${tone === 2 ? s.tone2 : ''}`} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden={label ? undefined : 'true'} role={label ? 'img' : undefined} aria-label={label}>
      <path class={s.area} d={area} />
      <path class={s.line} d={line} vector-effect="non-scaling-stroke" />
      <circle class={s.dot} cx={last[0]} cy={last[1]} r="2.5" vector-effect="non-scaling-stroke" />
    </svg>
  );
}

/** A ring showing a share (0–100), for compact cards. */
export function Ring({ value, size = 56, tone = 'accent' }: { value: number; size?: number; tone?: 'accent' | 'warn' | 'danger' }) {
  const v = useTween(value);
  const r = 15.5;
  const c = 2 * Math.PI * r;
  return (
    <svg class={`${s.ring} ${s[tone]}`} width={size} height={size} viewBox="0 0 36 36" aria-hidden="true">
      <circle class={s.track} cx="18" cy="18" r={r} />
      <circle class={s.value} cx="18" cy="18" r={r} stroke-dasharray={`${(Math.max(0, Math.min(100, v)) / 100) * c} ${c}`} />
    </svg>
  );
}
