// The web terminal: xterm over /ws/v2/terminal (binary frames both ways;
// a text frame resizes). xterm is in this page's chunk only (D31).
import '@xterm/xterm/css/xterm.css';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { useEffect, useRef, useState } from 'preact/hooks';
import { Plug } from 'lucide-preact';
import { wsUrl } from '../../api/client';
import { t } from '../../text/i18n';
import { Button } from '../../kit/Button';
import { Page } from '../../kit/Page';
import { Status } from '../../kit/Status';
import s from './Tools.module.css';

function themeFromTokens() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return { background: v('--surface-sunken'), foreground: v('--text'), cursor: v('--accent'), selectionBackground: v('--accent-soft-2') };
}

type State = 'connecting' | 'open' | 'closed';

export default function Terminal() {
  const box = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<State>('connecting');
  const [session, setSession] = useState(0);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const term = new XTerm({
      cursorBlink: true,
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue('--font-mono'),
      fontSize: 13,
      scrollback: 2000,
      theme: themeFromTokens(),
      allowProposedApi: false,
      // Colours from programs are adjusted to stay readable (WCAG AA).
      minimumContrastRatio: 4.5,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();
    setState('connecting');
    const ws = new WebSocket(wsUrl('/ws/v2/terminal'));
    ws.binaryType = 'arraybuffer';
    const enc = new TextEncoder();
    const resize = () => {
      fit.fit();
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
    };
    ws.onopen = () => {
      setState('open');
      resize();
      term.focus();
    };
    ws.onmessage = (e) => term.write(typeof e.data === 'string' ? e.data : new Uint8Array(e.data as ArrayBuffer));
    ws.onclose = () => {
      setState('closed');
      term.write(`\r\n\x1b[2m${t('terminal.closed')}\x1b[0m\r\n`);
    };
    const input = term.onData((d) => ws.readyState === WebSocket.OPEN && ws.send(enc.encode(d)));
    const ro = new ResizeObserver(() => resize());
    ro.observe(el);
    // Follow the theme when it changes.
    const mo = new MutationObserver(() => (term.options.theme = themeFromTokens()));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => {
      ro.disconnect();
      mo.disconnect();
      input.dispose();
      ws.onclose = null;
      ws.close();
      term.dispose();
    };
  }, [session]);

  return (
    <Page
      title={t('nav.terminal')}
      subtitle={t('terminal.subtitle')}
      actions={
        <>
          {state === 'open' ? <Status tone="ok">{t('terminal.connected')}</Status> : state === 'connecting' ? <Status tone="busy">{t('terminal.connecting')}</Status> : <Status tone="neutral">{t('terminal.disconnected')}</Status>}
          {state === 'closed' && (
            <Button icon={Plug} variant="primary" onClick={() => setSession((n) => n + 1)}>
              {t('terminal.reconnect')}
            </Button>
          )}
        </>
      }
    >
      <div class={s.terminal} ref={box} role="application" aria-label={t('nav.terminal')} />
    </Page>
  );
}
