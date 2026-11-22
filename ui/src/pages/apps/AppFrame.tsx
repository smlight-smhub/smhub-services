// An app's own web page inside the hub's frame. The iframe points at the
// app's port on this host, so it never shares the hub's origin (D43). While
// the app starts, a friendly state is shown instead of a broken page.
import { useEffect, useRef, useState } from 'preact/hooks';
import { useParams } from 'wouter-preact';
import { ExternalLink, Info, Play, RotateCw, ScrollText } from 'lucide-preact';
import { t } from '../../text/i18n';
import { appCommand, appWebUrl } from '../../state/apps';
import { useLive } from '../../state/store';
import { AppIcon, appTitle } from '../../kit/Bits';
import { Button, IconButton, LinkButton } from '../../kit/Button';
import { EmptyState, Spinner } from '../../kit/Feedback';
import { useDocumentTitle, useHeadingFocus } from '../../kit/Page';
import s from './Apps.module.css';

const WAIT_MS = 120_000;

export default function AppFrame() {
  const { name } = useParams<{ name: string }>();
  const apps = useLive('apps');
  const a = apps?.apps.find((x) => x.name === name);
  const title = appTitle(name);
  const [loaded, setLoaded] = useState(false);
  const [waitedOut, setWaitedOut] = useState(false);
  const [busy, setBusy] = useState(false);
  const [frameKey, setFrameKey] = useState(0);
  const h1 = useRef<HTMLHeadingElement>(null);
  useDocumentTitle(title);
  useHeadingFocus(h1);

  const running = !!a && (!a.is_service || a.state === 'started');
  const starting = a?.state === 'starting';
  useEffect(() => {
    setLoaded(false);
    setWaitedOut(false);
    if (running) return;
    const id = setTimeout(() => setWaitedOut(true), WAIT_MS);
    return () => clearTimeout(id);
  }, [running, name]);

  const url = a ? appWebUrl(a) : null;
  const blocked = location.protocol === 'https:' && !!a?.web && !a.web.https_port;

  let overlay = null;
  if (!apps) overlay = <Spinner label={t('common.loading')} />;
  else if (!a || !a.installed)
    overlay = (
      <EmptyState title={t('frame.notInstalled', { app: title })} action={<LinkButton href={`/apps/${name}`}>{t('frame.toStore')}</LinkButton>}>
        {t('frame.notInstalledText')}
      </EmptyState>
    );
  else if (!url)
    overlay = (
      <EmptyState title={t('frame.noWeb', { app: title })} action={<LinkButton href={`/apps/${name}`}>{t('frame.appPage')}</LinkButton>} />
    );
  else if (blocked)
    overlay = (
      <EmptyState
        icon={Info}
        title={t('frame.httpsTitle')}
        action={
          <LinkButton href={url} external variant="primary" icon={ExternalLink}>
            {t('frame.newTab')}
          </LinkButton>
        }
      >
        {t('frame.httpsText', { app: title })}
      </EmptyState>
    );
  else if (!running)
    overlay = (
      <div class={s.frameMessage}>
        <AppIcon name={name} src={a.icon} size={64} />
        {(starting || a.state === 'stopping') && !waitedOut ? (
          <>
            <Spinner />
            <p role="status">{starting ? t('frame.starting', { app: title }) : t('state.stoppingNamed', { name: title })}</p>
          </>
        ) : (
          <>
            <p role="status">
              <strong>{waitedOut ? t('frame.slow', { app: title }) : t('frame.stopped', { app: title })}</strong>
            </p>
            <div class={s.tileActions}>
              <Button
                variant="primary"
                icon={Play}
                busy={busy}
                onClick={async () => {
                  setBusy(true);
                  await appCommand(name, 'start');
                  setBusy(false);
                }}
              >
                {t('apps.start')}
              </Button>
              <LinkButton href={`/apps/${name}?tab=log`} icon={ScrollText}>
                {t('frame.showLog')}
              </LinkButton>
            </div>
          </>
        )}
      </div>
    );

  return (
    <div class={s.frameShell}>
      <div class={s.frameBar}>
        <h1 ref={h1} tabIndex={-1} class={s.frameTitle}>
          {title}
        </h1>
        {url && running && <IconButton icon={RotateCw} label={t('frame.reload')} size="sm" onClick={() => setFrameKey((k) => k + 1)} />}
        <LinkButton href={`/apps/${name}`} size="sm" variant="ghost" icon={Info} label={t('frame.appPage')} />
        {url && <LinkButton href={url} external size="sm" variant="ghost" icon={ExternalLink} label={t('frame.newTab')} />}
      </div>
      <div class={s.frameWrap}>
        {url && running && !blocked && (
          <iframe
            key={frameKey}
            class={s.frame}
            src={url}
            title={t('frame.title', { app: title })}
            referrerpolicy="no-referrer"
            allow="clipboard-read; clipboard-write; fullscreen"
            onLoad={() => setLoaded(true)}
          />
        )}
        {(overlay || (running && !loaded)) && (
          <div class={s.frameOverlay}>
            {overlay ?? (
              <div class={s.frameMessage}>
                <AppIcon name={name} src={a?.icon} size={64} />
                <Spinner />
                <p role="status">{t('frame.opening', { app: title })}</p>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
