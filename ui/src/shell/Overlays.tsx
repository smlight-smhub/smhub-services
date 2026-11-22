// What the shell shows over or above every page: notices (connection, restart
// needed, open hub), the operations tray, toasts and the confirm dialog.
import { useEffect, useRef, useState } from 'preact/hooks';
import { Link } from 'wouter-preact';
import { CircleCheck, CircleX, Info, LockOpen, RotateCw, TriangleExclamationPoint, Undo2, X } from 'lucide-preact';
import type { Operation } from '../api/types';
import { api } from '../api/client';
import { t } from '../text/i18n';
import { list, percent, relative } from '../text/format';
import { confirmRequest } from '../state/confirm';
import { removeRestartNotice, restartNotices, type RestartNotice } from '../state/notices';
import { operations, operationStep, operationTitle } from '../state/operations';
import { power, restartHub } from '../state/power';
import { errorText } from '../state/save';
import { session } from '../state/session';
import { connection } from '../state/store';
import { hubTime } from '../state/system';
import { dismissToast, toast, toasts, type Toast } from '../state/toasts';
import { Button, IconButton } from '../kit/Button';
import { appTitle } from '../kit/Bits';
import { Dialog } from '../kit/Dialog';
import { Banner, ProgressBar } from '../kit/Feedback';
import { Ring } from '../kit/Live';
import { cx } from '../kit/cx';
import s from './Overlays.module.css';

const OPEN_NOTICE_KEY = 'smhub.openNoticeDismissed';

async function restartOne(n: RestartNotice) {
  if (n.kind === 'hub') {
    await restartHub();
    return;
  }
  try {
    if (n.kind === 'app' && n.target) await api.post('/apps/{name}/{action}', { path: { name: n.target, action: 'restart' } });
    else if (n.kind === 'esphome') await api.post('/settings/esphome/restart');
    removeRestartNotice(n.id);
    toast({ text: t('restart.done'), tone: 'ok' });
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
  }
}

function RestartBar() {
  const items = restartNotices.value;
  const [busy, setBusy] = useState<string>();
  if (!items.length) return null;
  // One button per thing to restart; the hub's restart covers everything.
  const hub = items.find((n) => n.kind === 'hub');
  const targets = hub ? [hub] : items.filter((n, i) => items.findIndex((m) => m.kind === n.kind && m.target === n.target) === i);
  const label = (n: RestartNotice) => (n.kind === 'hub' ? t('restart.hub') : n.kind === 'esphome' ? t('restart.esphome') : t('restart.app', { app: appTitle(n.target ?? '') }));
  return (
    <Banner
      tone="warn"
      icon={RotateCw}
      title={t('restart.title')}
      live
      action={
        <>
          {targets.map((n) => (
            <Button
              key={n.id}
              size="sm"
              variant="primary"
              busy={busy === n.id}
              onClick={async () => {
                setBusy(n.id);
                await restartOne(n);
                setBusy(undefined);
              }}
            >
              {label(n)}
            </Button>
          ))}
          <Button size="sm" variant="ghost" onClick={() => items.forEach((n) => removeRestartNotice(n.id))}>
            {t('restart.later')}
          </Button>
        </>
      }
    >
      {list(items.map((n) => n.reason))}
    </Banner>
  );
}

export function Notices({ loc }: { loc: string }) {
  const c = connection.value;
  const p = power.value;
  const [lost, setLost] = useState(false);
  const [openDismissed, setOpenDismissed] = useState(() => sessionStorage.getItem(OPEN_NOTICE_KEY) === '1');
  useEffect(() => {
    if (c === 'open') {
      setLost(false);
      return;
    }
    // A short blip is not worth a message.
    const id = setTimeout(() => setLost(true), 2500);
    return () => clearTimeout(id);
  }, [c]);
  const open = session.value?.mode === 'open' && !openDismissed && loc !== '/settings/accounts';
  const showConn = p === 'off' || p === 'restarting' || lost;
  if (!showConn && !restartNotices.value.length && !open) return null;
  return (
    <div class={s.notices}>
      {showConn && (
        <Banner tone={p === 'off' ? 'info' : 'warn'} live title={p === 'off' ? t('power.offNow') : p === 'restarting' ? t('power.restarting') : t('conn.lostTitle')}>
          {p === 'off' ? t('power.offNowText') : p === 'restarting' ? t('power.restartingText') : t('conn.lostText')}
        </Banner>
      )}
      <RestartBar />
      {open && (
        <Banner
          tone="info"
          icon={LockOpen}
          title={t('openHub.title')}
          action={
            <>
              <Link href="/settings/accounts" class={s.linkButton}>
                {t('openHub.action')}
              </Link>
              <IconButton
                icon={X}
                size="sm"
                label={t('common.dismiss')}
                onClick={() => {
                  sessionStorage.setItem(OPEN_NOTICE_KEY, '1');
                  setOpenDismissed(true);
                }}
              />
            </>
          }
        >
          {t('openHub.text')}
        </Banner>
      )}
    </div>
  );
}

/** Finished tasks stay in the tray this long; their toast has already told the user. */
const RECENT_MS = 60_000;

function OpRow({ op }: { op: Operation }) {
  const step = operationStep(op);
  const running = op.state === 'running';
  return (
    <li class={s.op}>
      <div class={s.opHead}>
        {op.state === 'succeeded' ? (
          <CircleCheck size={20} class={s.okIcon} aria-hidden="true" />
        ) : op.state === 'failed' ? (
          <CircleX size={20} class={s.badIcon} aria-hidden="true" />
        ) : (
          <span class={s.opSpinner} aria-hidden="true" />
        )}
        <span class={s.opTitle}>{operationTitle(op)}</span>
        {running && op.progress !== null && op.progress !== undefined && <span class="num">{percent(op.progress)}</span>}
      </div>
      {running && <ProgressBar value={op.progress} label={operationTitle(op)} />}
      {step && <p class={cx(s.opStep, op.state === 'failed' && s.opError)}>{step}</p>}
      <p class={s.opTime}>{relative(op.finished_at ?? op.started_at, hubTime(Date.now()))}</p>
      {op.state === 'succeeded' && op.reboot_needed && (
        <Button size="sm" variant="primary" icon={RotateCw} onClick={() => void restartHub()}>
          {t('restart.hub')}
        </Button>
      )}
    </li>
  );
}

export function OpsTray() {
  const ops = operations.value;
  const [open, setOpen] = useState(false);
  const [cleared, setCleared] = useState<Set<string>>(() => new Set());
  const now = hubTime(Date.now());
  const running = ops.filter((o) => o.state === 'running');
  const recent = ops.filter((o) => o.state !== 'running' && !cleared.has(o.id) && o.finished_at && now - Date.parse(o.finished_at) < RECENT_MS);
  const shown = [...running, ...recent];
  // Hide finished tasks when their time is up, without a ticking clock.
  const [, expire] = useState(0);
  const nextExpiry = recent.length ? Math.min(...recent.map((o) => Date.parse(o.finished_at!) + RECENT_MS)) - now : 0;
  useEffect(() => {
    if (!nextExpiry) return;
    const id = setTimeout(() => expire((x) => x + 1), Math.max(1000, nextExpiry + 100));
    return () => clearTimeout(id);
  }, [nextExpiry > 0, recent.length]);
  if (!shown.length) return null;
  const main = shown[0]!;
  const failed = !running.length && recent.some((o) => o.state === 'failed');
  return (
    <>
      <button type="button" class={cx(s.tray, failed && s.trayFailed)} onClick={() => setOpen(true)} aria-haspopup="dialog">
        {running.length ? (
          main.progress === null || main.progress === undefined ? <span class={s.opSpinner} aria-hidden="true" /> : <Ring value={main.progress} size={26} />
        ) : failed ? (
          <TriangleExclamationPoint size={20} aria-hidden="true" />
        ) : (
          <CircleCheck size={20} aria-hidden="true" />
        )}
        <span class={s.trayText}>
          {running.length ? operationTitle(main) : t('ops.finished', { count: recent.length })}
          {running.length > 1 && <span class={s.more}> {t('ops.more', { count: running.length - 1 })}</span>}
        </span>
        {running.length > 0 && main.progress !== null && main.progress !== undefined && <span class="num">{percent(main.progress)}</span>}
      </button>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title={t('ops.title')}
        description={t('ops.description')}
        footer={
          recent.length > 0 && (
            <Button
              variant="ghost"
              onClick={() => {
                setCleared(new Set([...cleared, ...recent.map((o) => o.id)]));
                if (!running.length) setOpen(false);
              }}
            >
              {t('ops.clear')}
            </Button>
          )
        }
      >
        <ul class={s.ops}>
          {shown.map((o) => (
            <OpRow key={o.id} op={o} />
          ))}
        </ul>
      </Dialog>
    </>
  );
}

const TOAST_ICON = { ok: CircleCheck, info: Info, warn: TriangleExclamationPoint, danger: CircleX };

function ToastItem({ item }: { item: Toast }) {
  const [paused, setPaused] = useState(false);
  const left = useRef(item.timeout);
  useEffect(() => {
    if (!item.timeout || paused) return;
    const start = Date.now();
    const id = setTimeout(() => dismissToast(item.id), left.current);
    return () => {
      clearTimeout(id);
      left.current -= Date.now() - start;
    };
  }, [paused, item.id, item.timeout]);
  const I = TOAST_ICON[item.tone];
  return (
    <li
      class={cx(s.toast, s[item.tone])}
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
      onFocusIn={() => setPaused(true)}
      onFocusOut={() => setPaused(false)}
    >
      <I size={20} class={s.toastIcon} aria-hidden="true" />
      {/* Errors are announced at once; the list item keeps its role. */}
      <span class={s.toastText} role={item.tone === 'danger' ? 'alert' : undefined}>
        {item.text}
      </span>
      {item.undo && (
        <Button
          size="sm"
          variant="ghost"
          icon={Undo2}
          class={s.toastAction}
          onClick={() => {
            dismissToast(item.id);
            void item.undo?.();
          }}
        >
          {t('common.undo')}
        </Button>
      )}
      {item.action && (
        <Button
          size="sm"
          variant="ghost"
          class={s.toastAction}
          onClick={() => {
            dismissToast(item.id);
            void item.action?.run();
          }}
        >
          {item.action.label}
        </Button>
      )}
      <IconButton icon={X} size="sm" label={t('common.dismiss')} onClick={() => dismissToast(item.id)} class={s.toastClose} />
    </li>
  );
}

export function Toasts() {
  return (
    <section class={s.toasts} aria-label={t('toast.region')}>
      <ol aria-live="polite" aria-relevant="additions">
        {toasts.value.map((x) => (
          <ToastItem key={x.id} item={x} />
        ))}
      </ol>
    </section>
  );
}

export function ConfirmHost() {
  const r = confirmRequest.value;
  return (
    <Dialog
      open={!!r}
      onClose={() => r?.resolve(false)}
      title={r?.title ?? ''}
      size="sm"
      footer={
        r && (
          <>
            <Button autoFocus onClick={() => r.resolve(false)}>
              {r.cancel ?? t('common.cancel')}
            </Button>
            <Button variant={r.tone === 'danger' ? 'danger' : 'primary'} onClick={() => r.resolve(true)}>
              {r.confirm}
            </Button>
          </>
        )
      }
    >
      {r?.body && <div class={s.confirmBody}>{r.body}</div>}
    </Dialog>
  );
}
