// Each page is its own chunk, loaded when first opened (D31). Hovering or
// focusing a link starts the download early, so the page opens at once.
import type { ComponentType, FunctionComponent } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { RefreshCw } from 'lucide-preact';
import { t } from '../text/i18n';
import { Button } from '../kit/Button';
import { Banner, Skeleton } from '../kit/Feedback';
import s from './Shell.module.css';

export type Loader = () => Promise<{ default: ComponentType<object> }>;

export type LazyComponent = FunctionComponent<object> & {
  preload: () => Promise<void>;
};

export function lazy(load: Loader, fallback: () => preact.JSX.Element | null = PageLoading): LazyComponent {
  const Fallback = fallback;
  let Loaded: ComponentType<object> | undefined;
  let pending: Promise<void> | undefined;
  let failed = false;
  const preload = () =>
    (pending ??= load().then(
      (m) => {
        Loaded = m.default;
        failed = false;
      },
      (e: unknown) => {
        failed = true;
        pending = undefined;
        throw e;
      },
    ));
  function Lazy(props: object) {
    const [, rerender] = useState(0);
    useEffect(() => {
      if (Loaded) return;
      preload().then(
        () => rerender((x) => x + 1),
        () => rerender((x) => x + 1),
      );
    }, []);
    if (Loaded) return <Loaded {...props} />;
    if (failed) return <ChunkFailed />;
    return <Fallback />;
  }
  Lazy.preload = () => preload().catch(() => undefined);
  return Lazy as LazyComponent;
}

/** Shown only if a page takes a moment to arrive (the skeleton fades in late). */
export function PageLoading() {
  return (
    <div class={s.pageLoading} role="status" aria-live="polite">
      <span class="visually-hidden">{t('common.loading')}</span>
      <Skeleton height={36} width="40%" />
      <Skeleton height={120} />
      <Skeleton height={120} />
    </div>
  );
}

function ChunkFailed() {
  return (
    <div class={s.pageLoading}>
      <Banner
        tone="warn"
        title={t('error.chunkTitle')}
        action={
          <Button icon={RefreshCw} onClick={() => location.reload()}>
            {t('common.reload')}
          </Button>
        }
      >
        {t('error.chunkText')}
      </Banner>
    </div>
  );
}
