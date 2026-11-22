// Loaded on demand by Markdown.tsx.
import { marked } from 'marked';
import DOMPurify from 'dompurify';

DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener noreferrer');
  }
});

/** Markdown to sanitised HTML. Images, styles, frames and forms are removed: nothing loads from outside. */
export function render(md: string): string {
  const html = marked.parse(md, { async: false, gfm: true, breaks: false });
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ['img', 'style', 'iframe', 'form', 'input', 'button', 'video', 'audio', 'picture', 'source', 'svg', 'math'],
    FORBID_ATTR: ['style', 'srcset'],
  });
}
