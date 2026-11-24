import { memo, type ReactNode } from 'react';
import { parseInline, parseMarkdown, type Block, type Inline } from './markdown-parse';
import { CopyButton } from './common';

// Styles: web/src/styles/markdown.css (`.markdown`, `.md-code`, `.md-table`).

function renderInline(nodes: Inline[], key = ''): ReactNode[] {
  // biome-ignore lint/suspicious/useIterableCallbackReturn: the switch covers every `Inline` kind, so every path returns.
  return nodes.map((n, i) => {
    const k = `${key}${i}`;
    switch (n.t) {
      case 'text':
        return n.v;
      case 'code':
        return <code key={k}>{n.v}</code>;
      case 'strong':
        return <strong key={k}>{renderInline(n.c, k)}</strong>;
      case 'em':
        return <em key={k}>{renderInline(n.c, k)}</em>;
      case 'link':
        return (
          <a key={k} href={n.href} target="_blank" rel="noreferrer noopener">
            {renderInline(n.c, k)}
          </a>
        );
    }
  });
}

function renderBlock(b: Block, i: number): ReactNode {
  switch (b.t) {
    case 'p':
      return <p key={i}>{renderInline(b.c)}</p>;
    case 'h': {
      const level = Math.min(6, b.level + 2);
      const Tag = `h${level}` as 'h3';
      return <Tag key={i}>{renderInline(b.c)}</Tag>;
    }
    case 'code':
      return (
        <div key={i} className="md-code">
          <div className="md-code-bar">
            <span>{b.lang || 'text'}</span>
            <CopyButton text={b.v} label="Copy code" />
          </div>
          <pre>
            <code>{b.v}</code>
          </pre>
        </div>
      );
    case 'ul':
    case 'ol': {
      const List = b.t;
      return (
        <List key={i} start={b.t === 'ol' ? b.start : undefined}>
          {b.items.map((item, j) => (
            <li key={j}>{renderInline(item)}</li>
          ))}
        </List>
      );
    }
    case 'quote':
      return <blockquote key={i}>{b.c.map(renderBlock)}</blockquote>;
    case 'hr':
      return <hr key={i} />;
    case 'table':
      return (
        <div key={i} className="md-table">
          <table>
            <thead>
              <tr>
                {b.rows[0]?.map((cell, j) => (
                  <th key={j}>{renderInline(cell)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.slice(1).map((row, r) => (
                <tr key={r}>
                  {row.map((cell, j) => (
                    <td key={j}>{renderInline(cell)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}

/** Agent text as safe Markdown-lite (no raw HTML); `inline` renders one line in a <span>. */
export const Markdown = memo(function Markdown({ text, className = '', inline = false }: { text: string; className?: string; inline?: boolean }) {
  if (inline) return <span className={`markdown-inline ${className}`}>{renderInline(parseInline(text))}</span>;
  return <div className={`markdown ${className}`}>{parseMarkdown(text).map(renderBlock)}</div>;
});
