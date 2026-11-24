import { useState } from 'react';

type LongTextProps = {
  text: string;
  className?: string;
  /** Collapse when the text has more lines than this … */
  maxLines?: number;
  /** … or more characters than this. */
  maxChars?: number;
  /** Monospace `<pre>` (default) or wrapped prose. */
  mono?: boolean;
};

/** A `<pre>` (or paragraph) that can be collapsed when long. */
export function LongText({ text, className = '', maxLines = 14, maxChars = 1600, mono = true }: LongTextProps) {
  const [open, setOpen] = useState(false);
  const lines = text.split('\n');
  const long = lines.length > maxLines || text.length > maxChars;
  const collapsed = long && !open;
  const shown = collapsed ? lines.slice(0, maxLines).join('\n').slice(0, maxChars) : text;
  return (
    <div className={`long-text ${className}`}>
      {mono ? (
        <pre>
          {shown}
          {collapsed ? '\n…' : ''}
        </pre>
      ) : (
        <p className="prewrap">
          {shown}
          {collapsed ? '…' : ''}
        </p>
      )}
      {long && (
        <button type="button" className="text-button" onClick={() => setOpen(!open)}>
          {open ? 'Show less' : `Show all (${lines.length.toLocaleString()} lines, ${text.length.toLocaleString()} characters)`}
        </button>
      )}
    </div>
  );
}
