// Changelogs and package text in Markdown. `marked` and `dompurify` are loaded
// only when the first one is shown (D31); the HTML is always sanitised (D43).
import { useEffect, useState } from 'preact/hooks';
import { Loading } from './Feedback';
import s from './Markdown.module.css';

let renderer: Promise<typeof import('./markdown-render')> | undefined;

export function Markdown({ text }: { text: string | null | undefined }) {
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    if (!text) {
      setHtml('');
      return;
    }
    renderer ??= import('./markdown-render');
    renderer
      .then((m) => alive && setHtml(m.render(text)))
      .catch(() => alive && setHtml(null));
    return () => {
      alive = false;
    };
  }, [text]);
  if (html === null) return <Loading lines={4} />;
  return <div class={s.md} dangerouslySetInnerHTML={{ __html: html }} />;
}
