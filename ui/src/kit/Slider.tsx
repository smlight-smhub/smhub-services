// A labelled range slider (the browser's own). It reports while it moves
// (`onInput`) and once when let go (`onValue`), so a page can preview a change
// and save only the last value.
import { useId } from 'preact/hooks';
import s from './Slider.module.css';

export function Slider({
  label,
  value,
  min = 0,
  max = 100,
  step = 1,
  valueText,
  onValue,
  onInput,
  disabled,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  /** The value as people read it, e.g. "40 %". */
  valueText: string;
  onValue: (v: number) => void;
  onInput?: (v: number) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div class={s.slider}>
      <div class={s.head}>
        <label for={id} class={s.label}>
          {label}
        </label>
        <output for={id} class={`${s.value} num`}>
          {valueText}
        </output>
      </div>
      <input
        id={id}
        type="range"
        class={s.range}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        aria-valuetext={valueText}
        onInput={(e) => onInput?.(Number((e.currentTarget as HTMLInputElement).value))}
        onChange={(e) => onValue(Number((e.currentTarget as HTMLInputElement).value))}
      />
    </div>
  );
}
