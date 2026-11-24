import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseInline, parseMarkdown } from './markdown-parse';

test('blocks: code fences, headings, lists, quotes, paragraphs', () => {
  const blocks = parseMarkdown('# Title\n\nSome **bold** text\nnext line\n\n- one\n- two `x`\n\n1. first\n2. second\n\n```ts\nconst a = 1;\n```\n> quoted');
  assert.deepEqual(
    blocks.map((b) => b.t),
    ['h', 'p', 'ul', 'ol', 'code', 'quote']
  );
  const code = blocks[4];
  assert.ok(code.t === 'code' && code.lang === 'ts' && code.v === 'const a = 1;');
});

test('inline: code, bold, italic, safe links only', () => {
  assert.deepEqual(parseInline('a `b` **c** *d*'), [
    { t: 'text', v: 'a ' },
    { t: 'code', v: 'b' },
    { t: 'text', v: ' ' },
    { t: 'strong', c: [{ t: 'text', v: 'c' }] },
    { t: 'text', v: ' ' },
    { t: 'em', c: [{ t: 'text', v: 'd' }] },
  ]);
  assert.deepEqual(parseInline('[x](https://a.b)'), [{ t: 'link', href: 'https://a.b', c: [{ t: 'text', v: 'x' }] }]);
  // javascript: links stay text.
  assert.deepEqual(
    parseInline('[x](javascript:alert(1))').map((n) => n.t),
    ['text']
  );
  // Python dunders and snake_case identifiers are not emphasis.
  assert.deepEqual(parseInline('textkit/__init__.py and __init__'), [{ t: 'text', v: 'textkit/__init__.py and __init__' }]);
  assert.deepEqual(parseInline('__Bold text__'), [{ t: 'strong', c: [{ t: 'text', v: 'Bold text' }] }]);
  // snake_case identifiers are not italic.
  assert.deepEqual(parseInline('change_attempts_exhausted'), [{ t: 'text', v: 'change_attempts_exhausted' }]);
});

test('raw HTML stays text', () => {
  const blocks = parseMarkdown('<img src=x onerror=alert(1)>');
  assert.deepEqual(blocks, [{ t: 'p', c: [{ t: 'text', v: '<img src=x onerror=alert(1)>' }] }]);
});

test('tables', () => {
  const blocks = parseMarkdown('| a | b |\n|---|---|\n| 1 | 2 |');
  assert.equal(blocks[0].t, 'table');
  assert.equal(blocks[0].t === 'table' && blocks[0].rows.length, 2);
});

test('underscores inside names are text, around words they are emphasis', () => {
  assert.deepEqual(parseInline('pkg/__main__ and snake__case__x'), [{ t: 'text', v: 'pkg/__main__ and snake__case__x' }]);
  assert.deepEqual(parseInline('__Init__.py'), [{ t: 'text', v: '__Init__.py' }]);
  assert.deepEqual(parseInline('**a_b**'), [{ t: 'strong', c: [{ t: 'text', v: 'a_b' }] }]);
  assert.deepEqual(parseInline('an _italic_ word'), [
    { t: 'text', v: 'an ' },
    { t: 'em', c: [{ t: 'text', v: 'italic' }] },
    { t: 'text', v: ' word' },
  ]);
  assert.deepEqual(parseInline('_x_y'), [{ t: 'text', v: '_x_y' }]);
  assert.deepEqual(parseInline('\\*not em\\*'), [{ t: 'text', v: '*not em*' }]);
  assert.deepEqual(parseInline('`a'), [{ t: 'text', v: '`a' }]);
});

test('paragraphs end at a block start; rules and tables do not interrupt them', () => {
  assert.deepEqual(
    parseMarkdown('para\n# heading\n> quote').map((b) => b.t),
    ['p', 'h', 'quote']
  );
  assert.deepEqual(parseMarkdown('para\n---\n| a |'), [{ t: 'p', c: [{ t: 'text', v: 'para\n---\n| a |' }] }]);
  // A `#` line that is not a heading (its text holds a line separator) stands alone.
  assert.deepEqual(
    parseMarkdown('# a b\nnext').map((b) => b.t),
    ['p', 'p']
  );
});

test('a table drops rule rows under its header row only', () => {
  const cells = (source: string) => {
    const [table] = parseMarkdown(source);
    return table.t === 'table' ? table.rows.map((r) => r.map((c) => c.map((n) => (n.t === 'text' ? n.v : n.t)).join(''))) : null;
  };
  assert.deepEqual(cells('| a |\n|---|\n| --- |\n| 1 |'), [['a'], ['1']]);
  assert.deepEqual(cells('| a |\n|---|\n| 1 |\n| --- |'), [['a'], ['1'], ['---']]);
});
