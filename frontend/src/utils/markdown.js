import DOMPurify from 'dompurify';
import { marked } from 'marked';

// Configure marked for security
marked.setOptions({
  gfm: true,
  breaks: false,
});

/**
 * Render markdown to sanitized HTML.
 * Prevents XSS from user-supplied README content.
 */
export function renderMarkdown(md) {
  if (!md) return '';
  const raw = marked.parse(md);
  return DOMPurify.sanitize(raw, {
    ALLOWED_TAGS: [
      'h1',
      'h2',
      'h3',
      'h4',
      'h5',
      'h6',
      'p',
      'br',
      'hr',
      'strong',
      'em',
      'del',
      's',
      'a',
      'img',
      'ul',
      'ol',
      'li',
      'code',
      'pre',
      'blockquote',
      'table',
      'thead',
      'tbody',
      'tr',
      'th',
      'td',
      'div',
      'span',
      'details',
      'summary',
      'sup',
      'sub',
    ],
    ALLOWED_ATTR: [
      'href',
      'src',
      'alt',
      'title',
      'class',
      'id',
      'target',
      'rel',
    ],
    ADD_ATTR: ['target'],
    FORBID_TAGS: [
      'style',
      'script',
      'iframe',
      'object',
      'embed',
      'form',
      'input',
    ],
  });
}
