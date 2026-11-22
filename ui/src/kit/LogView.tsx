// A log: the last lines from the hub, then new lines live over
// /ws/v2/logs/{name}. The list is capped, so a long-open tab does not grow.
import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { ArrowDown, Download, Pause, Play, Search, Trash } from 'lucide-preact';
import { api, apiUrl, wsUrl } from '../api/client';
import { t } from '../text/i18n';
import { num } from '../text/format';
import { Button, IconButton, LinkButton } from './Button';
import { TextField } from './Field';
import { ErrorState } from './Feedback';
import { cx } from './cx';
import s from './LogView.module.css';

const MAX_LINES = 2000;
const HISTORY = 500;

type Live = 'off' | 'connecting' | 'on' | 'stopped';

function level(line: string): string | undefined {
  if (/\b(error|err|fatal|crit(ical)?|panic|exception|failed)\b/i.test(line)) return s.error;
  if (/\b(warn(ing)?)\b/i.test(line)) return s.warn;
  return undefined;
}

export function LogView({ name, height = 480 }: { name: string; height?: number }) {
  const [lines, setLines] = useState<string[]>([]);
  const [filter, setFilter] = useState('');
  const [applied, setApplied] = useState('');
  const [error, setError] = useState<unknown>();
  const [loading, setLoading] = useState(true);
  const [follow, setFollow] = useState(true);
  const [live, setLive] = useState<Live>('off');
  const [atEnd, setAtEnd] = useState(true);
  const box = useRef<HTMLDivElement>(null);
  const appliedRef = useRef('');
  appliedRef.current = applied;

  // Filter typed: applied after a short pause, to the hub's history and to live lines.
  useEffect(() => {
    const id = setTimeout(() => setApplied(filter.trim()), 350);
    return () => clearTimeout(id);
  }, [filter]);

  const loadHistory = (count = HISTORY) => {
    setLoading(true);
    api
      .get('/logs/{name}', { path: { name }, query: { lines: count, filter: applied || undefined } })
      .then((r) => {
        setLines(r.lines.slice(-MAX_LINES));
        setError(undefined);
      })
      .catch(setError)
      .finally(() => setLoading(false));
  };

  useEffect(() => loadHistory(), [name, applied]);

  useEffect(() => {
    if (!follow) {
      setLive('off');
      return;
    }
    setLive('connecting');
    const ws = new WebSocket(wsUrl(`/ws/v2/logs/${encodeURIComponent(name)}`));
    let buffer: string[] = [];
    let frame = 0;
    const flush = () => {
      frame = 0;
      const add = buffer;
      buffer = [];
      setLines((old) => {
        const next = old.concat(add);
        return next.length > MAX_LINES ? next.slice(-MAX_LINES) : next;
      });
    };
    ws.onopen = () => setLive('on');
    ws.onmessage = (e) => {
      if (typeof e.data !== 'string') return;
      let line: string;
      try {
        line = (JSON.parse(e.data) as { line?: string }).line ?? '';
      } catch {
        line = e.data;
      }
      const f = appliedRef.current.toLowerCase();
      if (f && !line.toLowerCase().includes(f)) return;
      buffer.push(line);
      // Many lines at once are drawn together, once per frame.
      if (!frame) frame = requestAnimationFrame(flush);
    };
    ws.onclose = () => setLive((l) => (l === 'off' ? l : 'stopped'));
    return () => {
      cancelAnimationFrame(frame);
      ws.onclose = null;
      ws.close();
    };
  }, [name, follow]);

  // Keep the newest line in view while the user is at the end.
  useEffect(() => {
    const el = box.current;
    if (el && atEnd) el.scrollTop = el.scrollHeight;
  }, [lines, atEnd]);

  const rendered = useMemo(
    () =>
      lines.map((l, i) => (
        <div key={i} class={cx(s.line, level(l))}>
          {l || ' '}
        </div>
      )),
    [lines],
  );

  return (
    <div class={s.wrap}>
      <div class={s.toolbar}>
        <TextField label={t('logs.filter')} hideLabel type="search" icon={Search} value={filter} onValue={setFilter} placeholder={t('logs.filterPlaceholder')} class={s.filter} />
        <div class={s.tools}>
          <Button size="sm" icon={follow ? Pause : Play} onClick={() => setFollow(!follow)} aria-pressed={follow}>
            {follow ? t('logs.pause') : t('logs.follow')}
          </Button>
          <IconButton size="sm" icon={Trash} label={t('logs.clear')} onClick={() => setLines([])} />
          <LinkButton size="sm" icon={Download} href={apiUrl('/logs/{name}/download', { name })} download={`${name}.log`} label={t('logs.download')} />
        </div>
      </div>
      <p class={s.status} role="status">
        {live === 'on' ? (
          <>
            <span class={s.liveDot} aria-hidden="true" />
            {t('logs.live')}
          </>
        ) : live === 'connecting' ? (
          t('logs.connecting')
        ) : live === 'stopped' ? (
          t('logs.stopped')
        ) : (
          t('logs.paused')
        )}
        <span class={s.count}>{t('logs.lines', { count: lines.length, n: num(lines.length) })}</span>
      </p>
      {error ? (
        <ErrorState error={error} retry={() => loadHistory()} />
      ) : (
        <div
          ref={box}
          class={s.box}
          style={{ blockSize: `${height}px` }}
          tabIndex={0}
          role="log"
          aria-label={t('logs.title', { name })}
          aria-busy={loading || undefined}
          onScroll={(e) => {
            const el = e.currentTarget as HTMLDivElement;
            setAtEnd(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
          }}
        >
          {!lines.length && !loading ? <div class={s.empty}>{applied ? t('logs.noMatch') : t('logs.empty')}</div> : rendered}
        </div>
      )}
      <div class={s.foot}>
        <Button size="sm" variant="ghost" onClick={() => loadHistory(5000)}>
          {t('logs.loadMore')}
        </Button>
        {!atEnd && (
          <Button size="sm" variant="soft" icon={ArrowDown} onClick={() => setAtEnd(true)}>
            {t('logs.toEnd')}
          </Button>
        )}
      </div>
    </div>
  );
}
