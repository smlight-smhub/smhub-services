// Ctrl+K / ⌘K opens the command palette (advanced level). The palette itself
// is a separate chunk, loaded the first time it opens.
import { signal } from '@preact/signals';
import { useEffect } from 'preact/hooks';
import { advanced } from '../state/prefs';
import { lazy } from './lazy';

export const paletteOpen = signal(false);

export function openPalette() {
  paletteOpen.value = true;
}

const Palette = lazy(() => import('./Palette'), () => null);

export function PaletteHost() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        if (!advanced.peek()) return;
        e.preventDefault();
        paletteOpen.value = !paletteOpen.peek();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  return paletteOpen.value ? <Palette /> : null;
}
