/**
 * A small, safe Markdown subset for agent messages: fenced code, headings, lists, quotes,
 * paragraphs, tables, and inline code / bold / italic / links. It produces a tree that
 * Markdown.tsx renders as elements; raw HTML is never interpreted.
 */
export type Inline = { t: 'text'; v: string } | { t: 'code'; v: string } | { t: 'strong'; c: Inline[] } | { t: 'em'; c: Inline[] } | { t: 'link'; href: string; c: Inline[] };

export type Block =
  | { t: 'p'; c: Inline[] }
  | { t: 'h'; level: number; c: Inline[] }
  | { t: 'code'; lang: string; v: string }
  | { t: 'ul'; items: Inline[][] }
  | { t: 'ol'; start: number; items: Inline[][] }
  | { t: 'quote'; c: Block[] }
  | { t: 'hr' }
  | { t: 'table'; rows: Inline[][][] };

// ---------- blocks ----------

/** ```` ```ts ```` or `~~~`: the marker (closed by the same) and the language. */
const FENCE = /^\s*(```+|~~~+)\s*([\w+-]*)/;
/** `## Title`: the level and the text. */
const HEADING = /^(#{1,6})\s+(.*)$/;
/** `---`, `***`, `___`, `- - -`. */
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s*>/;
/** `- item`: the indent and the text. */
const BULLET = /^(\s*)[-*+]\s+(.*)$/;
/** `1. item` or `1) item`: the indent, the number and the text. */
const ORDERED = /^(\s*)(\d+)[.)]\s+(.*)$/;
/** `| a | b |`: a table row (the header row too). */
const TABLE_ROW = /^\s*\|.*\|\s*$/;
/** `|---|:--:|`: the rule under a table's header row. */
const TABLE_RULE = /^\s*\|?\s*:?-{2,}/;

/** The start of a heading (`HEADING` also needs the rest of the line to be text). */
const HEADING_START = /^#{1,6}\s/;

/** A line that ends a paragraph and starts a block of its own (rules and tables don't). */
function interruptsParagraph(line: string): boolean {
  return FENCE.test(line) || HEADING_START.test(line) || QUOTE.test(line) || BULLET.test(line) || ORDERED.test(line);
}

/** A table row's cells, outer pipes dropped. */
function splitRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\||\|$/g, '')
    .split('|')
    .map((cell) => cell.trim());
}

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1];
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trimStart().startsWith(marker)) body.push(lines[i++]);
      i++; // closing fence (or end)
      blocks.push({ t: 'code', lang: fence[2] ?? '', v: body.join('\n') });
      continue;
    }
    if (!line.trim()) {
      i++;
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ t: 'h', level: heading[1].length, c: parseInline(heading[2].replace(/\s+#+\s*$/, '')) });
      i++;
      continue;
    }
    if (RULE.test(line)) {
      blocks.push({ t: 'hr' });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) body.push(lines[i++].replace(/^\s*>\s?/, ''));
      blocks.push({ t: 'quote', c: parseMarkdown(body.join('\n')) });
      continue;
    }
    if (TABLE_ROW.test(line) && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1])) {
      const rows: Inline[][][] = [];
      for (; i < lines.length && TABLE_ROW.test(lines[i]); i++) {
        const isHeaderRule = rows.length === 1 && TABLE_RULE.test(lines[i]);
        if (!isHeaderRule) rows.push(splitRow(lines[i]).map((cell) => parseInline(cell)));
      }
      blocks.push({ t: 'table', rows });
      continue;
    }
    if (BULLET.test(line) || ORDERED.test(line)) {
      const isOrdered = !BULLET.test(line);
      const item = isOrdered ? ORDERED : BULLET;
      const start = isOrdered ? Number(ORDERED.exec(line)![2]) : 1;
      const items: string[] = [];
      while (i < lines.length) {
        const l = lines[i];
        const m = item.exec(l);
        if (m) {
          items.push(isOrdered ? m[3] : m[2]);
          i++;
        } else if (l.trim() && /^\s{2,}/.test(l) && items.length) {
          // An indented line continues the item above.
          items[items.length - 1] += `\n${l.trim()}`;
          i++;
        } else break;
      }
      const parsed = items.map((t) => parseInline(t));
      blocks.push(isOrdered ? { t: 'ol', start, items: parsed } : { t: 'ul', items: parsed });
      continue;
    }
    // A first line that starts like a block but did not parse as one (a heading whose text holds
    // a line separator) is a paragraph on its own.
    const para: string[] = [line];
    i++;
    if (!interruptsParagraph(line)) while (i < lines.length && lines[i].trim() && !interruptsParagraph(lines[i])) para.push(lines[i++]);
    blocks.push({ t: 'p', c: parseInline(para.join('\n')) });
  }
  return blocks;
}

// ---------- inline ----------

const SAFE_HREF = /^(https?:\/\/|mailto:)/i;
const ESCAPABLE = /[\\`*_[\]()#+\-.!|]/;

/**
 * A token read at `text[i]`, and where reading goes on. A `text` node is literal text, merged
 * into the text around it.
 */
type Token = { node: Inline; next: number } | null;
type Reader = (text: string, i: number) => Token;

/** `\*`: the escaped character, literally. */
function readEscape(text: string, i: number): Token {
  return i + 1 < text.length && ESCAPABLE.test(text[i + 1]) ? { node: { t: 'text', v: text[i + 1] }, next: i + 2 } : null;
}

/** `` `code` `` or ``` `` co`de `` ```: closed by the same number of backticks. */
function readCode(text: string, i: number): Token {
  let ticks = 1;
  while (text[i + ticks] === '`') ticks++;
  const end = text.indexOf('`'.repeat(ticks), i + ticks);
  if (end < 0) return null;
  return { node: { t: 'code', v: text.slice(i + ticks, end).replace(/^ (.*) $/, '$1') }, next: end + ticks };
}

/**
 * `__x__` inside a name or a path is part of it, not bold: `__init__.py`, `pkg/__main__`,
 * `snake__case`, and a bare lowercase dunder such as `__init__`. `open` and `close` are the
 * indexes of the two delimiters.
 */
function isIntrawordDouble(text: string, open: number, close: number): boolean {
  const dunder = /^[a-z0-9_]+$/.test(text.slice(open + 2, close));
  const before = text[open - 1] ?? '';
  const after = text.slice(close + 2, close + 4);
  return dunder || /[\w/]/.test(before) || /^\.?\w/.test(after);
}

/** `_x_` with a letter or digit just outside it is part of a snake_case name, not italic. */
function isIntrawordSingle(text: string, open: number, close: number): boolean {
  return /\w/.test(text[open - 1] ?? '') || /\w/.test(text[close + 1] ?? '');
}

/** `**bold**` / `__bold__`, else `*italic*` / `_italic_`. A run of underscores that opens neither
 * stays text as a whole, so `snake__case` keeps both. */
function readEmphasis(text: string, i: number): Token {
  const ch = text[i];
  if (text[i + 1] === ch) {
    const close = text.indexOf(ch + ch, i + 2);
    if (close > i + 2 && !(ch === '_' && isIntrawordDouble(text, i, close))) {
      return { node: { t: 'strong', c: parseInline(text.slice(i + 2, close)) }, next: close + 2 };
    }
    if (ch === '_') {
      let run = 0;
      while (text[i + run] === '_') run++;
      return { node: { t: 'text', v: '_'.repeat(run) }, next: i + run };
    }
  }
  if (!text[i + 1] || text[i + 1] === ' ') return null;
  const close = text.indexOf(ch, i + 1);
  if (close <= i + 1 || text[close - 1] === ' ' || (ch === '_' && isIntrawordSingle(text, i, close))) return null;
  return { node: { t: 'em', c: parseInline(text.slice(i + 1, close)) }, next: close + 1 };
}

/** `[text](https://…)`; any other scheme (`javascript:`) stays text. */
function readLink(text: string, i: number): Token {
  const close = text.indexOf('](', i);
  const end = close > 0 ? text.indexOf(')', close + 2) : -1;
  if (close <= i || end <= close) return null;
  const href = text.slice(close + 2, end).trim();
  if (!SAFE_HREF.test(href)) return null;
  return { node: { t: 'link', href, c: parseInline(text.slice(i + 1, close)) }, next: end + 1 };
}

/** The character that may start each token; any other character is text. */
const READERS: Record<string, Reader> = { '\\': readEscape, '`': readCode, '*': readEmphasis, _: readEmphasis, '[': readLink };

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let buffer = '';
  let i = 0;
  while (i < text.length) {
    const token = Object.hasOwn(READERS, text[i]) ? READERS[text[i]](text, i) : null;
    if (!token) {
      buffer += text[i++];
      continue;
    }
    if (token.node.t === 'text') buffer += token.node.v;
    else {
      if (buffer) out.push({ t: 'text', v: buffer });
      buffer = '';
      out.push(token.node);
    }
    i = token.next;
  }
  if (buffer) out.push({ t: 'text', v: buffer });
  return out;
}
