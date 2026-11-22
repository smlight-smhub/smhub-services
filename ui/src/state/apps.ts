// App commands shared by the dashboard and the app pages.
import { api } from '../api/client';
import type { AppDetail, AppSummary } from '../api/types';
import { t } from '../text/i18n';
import { appTitle } from '../text/names';
import { getCached } from './get';
import { errorText, save } from './save';
import { toast } from './toasts';
import { confirm } from './confirm';

const titleOf = appTitle;

/** Whether this page reaches the hub through Tailscale (its 100.64.0.0/10 addresses or a ts.net name). */
function viaTailscale(): boolean {
  const h = location.hostname;
  const m = /^100\.(\d+)\./.exec(h);
  return h.endsWith('.ts.net') || (!!m && Number(m[1]) >= 64 && Number(m[1]) <= 127);
}

/** Start, stop or restart an app. Stopping offers "Undo". */
export async function appCommand(name: string, action: 'start' | 'stop' | 'restart'): Promise<boolean> {
  // Tailscale is often how the user reaches the hub: "Undo" cannot help once it is gone.
  if (name === 'tailscale' && action !== 'start') {
    const ok = await confirm({
      title: t(action === 'stop' ? 'ts.stopTitle' : 'ts.restartTitle'),
      body: t(viaTailscale() ? 'ts.stopBodyHere' : 'ts.stopBody'),
      confirm: t(action === 'stop' ? 'apps.stop' : 'apps.restart'),
      tone: 'danger',
    });
    if (!ok) return false;
  }
  try {
    const answer = await api.post('/apps/{name}/{action}', { path: { name, action } });
    const detail = getCached('/apps/{name}', { name });
    if (answer && 'form' in answer && detail.peek()) detail.value = answer;
    const app = titleOf(name);
    const done = { start: 'apps.started', stop: 'apps.stopped', restart: 'apps.restarted' } as const;
    toast({
      text: t(done[action], { app }),
      tone: 'ok',
      undo: action === 'stop' ? () => appCommand(name, 'start') : action === 'start' ? () => appCommand(name, 'stop') : undefined,
    });
    return true;
  } catch (e) {
    toast({ text: t('apps.commandFailed', { app: titleOf(name), reason: errorText(e) }), tone: 'danger' });
    return false;
  }
}

type Options = { autostart?: boolean; watchdog?: boolean; sidebar?: boolean };

/** Changes how the hub runs an app (switches), with "Undo". */
export function setAppOptions(name: string, patch: Options, message: string, undoable = true): Promise<AppDetail | undefined> {
  const sig = getCached('/apps/{name}', { name });
  const before = sig.peek();
  return save<AppDetail>({
    target: sig,
    optimistic: (a) => ({ ...a, ...patch }),
    send: () => api.patch('/apps/{name}', { path: { name }, body: patch }),
    success: message,
    undo:
      undoable && before
        ? (): Promise<AppDetail | undefined> => {
            const back: Options = {};
            for (const k of Object.keys(patch) as (keyof Options)[]) back[k] = before[k] ?? undefined;
            return setAppOptions(name, back, t('common.undone'), false);
          }
        : undefined,
  });
}

/** The address of an app's own web page on this hub, matching how this page was opened. */
export function appWebUrl(a: Pick<AppSummary, 'web'>): string | null {
  if (!a.web) return null;
  const https = location.protocol === 'https:';
  const port = https && a.web.https_port ? a.web.https_port : a.web.port;
  return `${https && a.web.https_port ? 'https:' : 'http:'}//${location.hostname}:${port}/`;
}
