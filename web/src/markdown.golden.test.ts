// Goldens for markdown-parse.ts: the parse tree (what Markdown.tsx renders) of a table of tricky inputs
// and of every agent message, reasoning, feature spec, task and artifact the API snapshots hold.
import { apiBodies, checkGolden } from './testdata/golden';
import { test } from 'node:test';
import { parseInline, parseMarkdown } from './markdown-parse';
import type { FeatureDetail, TranscriptResponse } from './types';

const BLOCKS = [
  '',
  '\n\n\n',
  'plain paragraph',
  'line one\nline two\n\nsecond paragraph',
  'windows\r\nline\rendings',
  '# h1\n## h2 ##\n###### h6\n####### seven\n#nospace',
  '```\ncode\n```',
  '```ts title\nconst x = 1;\n\n```\nafter',
  '~~~py\nunclosed fence',
  '````\n```\ninner\n````',
  '  ```js\n  indented fence\n  ```',
  '---\n***\n___\n- - -\n--',
  '> quote\n> # heading in quote\n>\n> - item',
  '>no space\n> > nested',
  '- a\n- b\n  continued\n* c\n+ d',
  '1. one\n2) two\n10. ten',
  '3. starts at three\n4. four',
  '- item\n\n- after blank',
  '- item\nparagraph right after',
  'para\n- list right after',
  'para\n1. numbered right after',
  '| a | b |\n|---|:--:|\n| 1 | **2** |\n| 3 |',
  '| only header |\n| --- |',
  '| not | a table |\nno separator',
  'a | b\n--|--\n1 | 2',
  '|---|---|\n|---|---|\n| x | y |',
  '<script>alert(1)</script>\n<b>raw html</b>',
  '\\# not a heading\n\\- not a list',
  '   \n  # indented heading?\n',
];
const INLINES = [
  '',
  'plain',
  '`code` and ``double `tick` code`` and `unclosed',
  '` padded `',
  '**bold** __bold__ *em* _em_',
  '**unclosed bold',
  '*unclosed em',
  '* not em*',
  '*em *',
  '__init__.py and pkg/__main__ and snake__case and __x__.y',
  'a__b__c and __b__c',
  'snake_case_name and _leading and trailing_',
  '_em_word and word_em_',
  '***bold em***',
  '**bold with *em* inside**',
  '[link](https://example.com) [mail](mailto:a@b.c) [js](javascript:alert(1)) [rel](/path)',
  '[no close](https://x',
  '[outer [inner](https://a)](https://b)',
  '[text](https://example.com/a_(b))',
  '[](https://empty.text)',
  '[**bold link**](HTTPS://EXAMPLE.COM)',
  '\\*escaped\\* \\_x\\_ \\\\ \\a \\[not link\\](https://x)',
  'trailing backslash \\',
  '___',
  '____ four',
  '_ spaced _',
  'emoji 😀 **bold 😀**',
];

/** Every free text the snapshots hold that the UI renders as Markdown, without repeats. */
function realTexts(): [string, string][] {
  const seen = new Set<string>();
  const out: [string, string][] = [];
  const add = (name: string, text: string | null | undefined) => {
    if (!text || seen.has(text)) return;
    seen.add(text);
    out.push([name, text]);
  };
  for (const [n, t] of apiBodies<TranscriptResponse>('runs', /transcript-[^/]*\.json$/)) {
    add(`${n}:instructions`, t.instructions);
    t.transcript.items.forEach((item, i) => {
      if (item.kind !== 'tool') add(`${n}@${i}:${item.kind}`, item.text);
      if (item.detail) add(`${n}@${i}:${item.kind}.detail`, item.detail);
    });
  }
  for (const [n, d] of apiBodies<FeatureDetail>('features', /\/detail\.json$/)) for (const s of d.slices) add(`${n}/${s.id}:spec`, s.spec);
  for (const [n, t] of apiBodies<{ text: string }>('features', /\/task-[^/]*\.json$/)) add(n, t.text);
  for (const [n, a] of apiBodies<{ content: string }>('features', /\/artifact-[^/]*\.json$/)) add(n, a.content);
  return out;
}

test('golden: parseMarkdown over a table of tricky inputs', () => {
  checkGolden(
    'markdown/parseMarkdown-table',
    BLOCKS.map((s) => [s, parseMarkdown(s)])
  );
});

test('golden: parseMarkdown over every message, spec, task and artifact in the snapshots', () => {
  checkGolden('markdown/parseMarkdown-snapshots', Object.fromEntries(realTexts().map(([n, t]) => [n, parseMarkdown(t)])));
});

test('golden: parseInline over a table of tricky inputs', () => {
  checkGolden(
    'markdown/parseInline-table',
    INLINES.map((s) => [s, parseInline(s)])
  );
});

test('golden: parseInline over the first line of every text in the snapshots', () => {
  checkGolden('markdown/parseInline-snapshots', Object.fromEntries(realTexts().map(([n, t]) => [n, parseInline(t.split('\n')[0])])));
});
