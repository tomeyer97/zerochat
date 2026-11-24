// Goldens for transcript-model.ts over every transcript the API snapshots hold
// (`api/runs/**/transcript-<n>[-ledger].json`), plus tables of edge inputs.
import { apiBodies, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import {
  CHAT_PAGE,
  type ChatEntry,
  IDLE_AFTER_MS,
  chatEntries,
  commonDir,
  deniedPermission,
  editPreview,
  exitNote,
  firstLine,
  hasNativePrompt,
  isNotice,
  isResultTool,
  lastSeen,
  lastThinkingIndex,
  relativeTo,
  splitZeroshotPrompt,
  synthesizedPrompt,
  toolSummary,
  transcriptStats,
  visibleRange,
} from './transcript-model';
import type { Json, Tool, TranscriptItem, TranscriptResponse } from './types';

const transcripts = apiBodies<TranscriptResponse>('runs', /transcript-[^/]*\.json$/);
const byTranscript = <T>(f: (t: TranscriptResponse, items: TranscriptItem[]) => T) => Object.fromEntries(transcripts.map(([n, t]) => [n, f(t, t.transcript.items)]));
const tools: [string, Tool][] = transcripts.flatMap(([n, t]) => t.transcript.items.flatMap((item, i) => (item.tool ? [[`${n}@${i}`, item.tool] as [string, Tool]] : [])));
/** Absolute paths the tool calls name, as Chat.tsx collects them for commonDir. */
const toolPaths = (items: TranscriptItem[]) =>
  items.flatMap((i) => {
    const input = i.tool?.input as Record<string, Json> | undefined;
    return input && typeof input === 'object' ? (['file_path', 'path', 'notebook_path'] as const).map((k) => input[k]).filter((p): p is string => typeof p === 'string') : [];
  });

const TABLE_TOOLS: Pick<Tool, 'name' | 'input'>[] = [
  { name: '', input: null },
  { name: 'Bash', input: { command: 'npm test\nsecond line' } },
  { name: 'Bash', input: { description: 'run the tests' } },
  { name: 'Bash', input: { command: `  ${'x'.repeat(200)}  ` } },
  { name: 'Read', input: { path: 'src/a.ts' } },
  { name: 'Edit', input: { file_path: '/w/src/a.ts', old_string: 'a\nb\nc', new_string: 'a\nB\nc\nd' } },
  { name: 'Edit', input: { file_path: '/w/a', old_string: 'same', new_string: 'same' } },
  { name: 'Edit', input: { file_path: '/w/a', old_string: '', new_string: 'new' } },
  { name: 'Edit', input: { file_path: '/w/a', old_string: 1, new_string: 'x' } as unknown as Json },
  { name: 'MultiEdit', input: { file_path: '/w/a', edits: [{ old_string: 'a', new_string: 'b' }, { new_string: 'only new' }, 'junk'] } },
  { name: 'Write', input: { file_path: '/w/new.ts', content: 'line 1\nline 2\n' } },
  { name: 'Write', input: { path: '/w/p' } },
  { name: 'NotebookEdit', input: { notebook_path: '/w/n.ipynb' } },
  { name: 'Glob', input: { pattern: '**/*.ts' } },
  { name: 'Grep', input: { pattern: 'TODO', path: 'src' } },
  { name: 'Grep', input: { path: 'src' } },
  { name: 'WebFetch', input: { url: 'https://example.com' } },
  { name: 'WebSearch', input: { query: 'elk layout' } },
  { name: 'Task', input: { prompt: 'first\nsecond' } },
  { name: 'Agent', input: { description: 'explore' } },
  { name: 'TodoWrite', input: { todos: [1, 2, 3] } },
  { name: 'TodoWrite', input: { todos: 'x' } },
  { name: 'bash', input: { command: 'ls' } },
  { name: 'read_bash', input: { shellId: 's1' } },
  { name: 'write_bash', input: { input: 'y\n', shellId: 's1' } },
  { name: 'write_bash', input: { shellId: 's1' } },
  { name: 'stop_bash', input: { shellId: 's1' } },
  { name: 'view', input: { path: '/w/a' } },
  { name: 'edit', input: { path: '/w/a', old_str: 'x\ny', new_str: 'x\nz' } },
  { name: 'str_replace', input: { path: '/w/a', old_str: 'x', new_str: 'y' } },
  { name: 'create', input: { path: '/w/b', file_text: 'one\ntwo' } },
  { name: 'grep', input: { pattern: 'p' } },
  { name: 'rg', input: { pattern: 'p', path: 'd' } },
  { name: 'glob', input: { pattern: '*' } },
  { name: 'web_fetch', input: { url: 'https://example.com/x' } },
  { name: 'task', input: { description: 'sub' } },
  { name: 'StructuredOutput', input: { verdict: 'accepted' } },
  { name: 'mcp__x__y', input: { url: 'u', query: 'q' } },
  { name: 'Custom', input: 'a raw string input\nwith two lines' },
  { name: 'Custom', input: [1, 2] },
  { name: 'Custom', input: { other: 1 } },
];

test('golden: toolSummary over every tool call', () => {
  checkGolden('transcript-model/toolSummary', { transcripts: Object.fromEntries(tools.map(([n, t]) => [n, toolSummary(t)])), table: TABLE_TOOLS.map((t) => [t, toolSummary(t)]) });
});

test('golden: editPreview over every tool call', () => {
  const big = { name: 'Edit', input: { old_string: Array.from({ length: 600 }, (_, i) => `o${i}`).join('\n'), new_string: Array.from({ length: 600 }, (_, i) => `n${i}`).join('\n') } };
  checkGolden('transcript-model/editPreview', {
    transcripts: Object.fromEntries(tools.map(([n, t]) => [n, editPreview(t)])),
    table: TABLE_TOOLS.map((t) => [t, editPreview(t)]),
    // Above 250k comparisons the diff falls back to all-old-then-all-new: pinned by its shape only.
    hugeEdit: (() => {
      const lines = editPreview(big)!.split('\n');
      return { lines: lines.length, first: lines.slice(0, 2), last: lines.slice(-2), at600: lines.slice(599, 601) };
    })(),
  });
});

test('golden: exitNote, deniedPermission, isResultTool over every tool call', () => {
  checkGolden('transcript-model/exitNote', {
    transcripts: Object.fromEntries(tools.map(([n, t]) => [n, exitNote(t.result)])),
    table: [undefined, { isError: false }, { isError: false, exitCode: 0 }, { isError: false, exitCode: 2 }, { isError: true, exitCode: 2 }, { isError: false, exitCode: -1 }].map((r) => exitNote(r)),
  });
  checkGolden('transcript-model/deniedPermission', {
    transcripts: Object.fromEntries(tools.map(([n, t]) => [n, deniedPermission(t.permission)])),
    table: [
      undefined,
      { kind: 'shell' },
      { kind: 'shell', result: '' },
      { kind: 'shell', result: 'approved' },
      { kind: 'shell', result: 'approved-for-session' },
      { kind: 'write', result: 'denied-by-rules' },
      { kind: 'url', result: 'denied-interactively-by-user' },
    ].map((p) => deniedPermission(p)),
  });
  checkGolden('transcript-model/isResultTool', {
    transcripts: Object.fromEntries(tools.map(([n, t]) => [n, isResultTool(t.name)])),
    table: [undefined, '', 'StructuredOutput', 'structuredoutput', 'Bash'].map((n) => [String(n), isResultTool(n)]),
  });
});

/**
 * An entry, written compactly: `item-5 = 5` for `{kind: 'item', key: 'item-5', index: 5, item}`,
 * `tools-3 = 3,4,5` for a group. Each entry's item must be the very transcript item at its index
 * (a copy or another item would print `≠`), so nothing is lost by leaving the items out.
 */
function compactEntries(items: TranscriptItem[], entries: ChatEntry[]): string[] {
  return entries.map((e) =>
    e.kind === 'item' ? `${e.key} = ${e.index}${e.item === items[e.index] ? '' : ' ≠'}` : `${e.key} = ${e.items.map((x) => `${x.index}${x.item === items[x.index] ? '' : '≠'}`).join(',')}`
  );
}

test('golden: chatEntries over every transcript', () => {
  checkGolden('transcript-model/chatEntries', {
    default: byTranscript((_, items) => compactEntries(items, chatEntries(items))),
    minGroup1: byTranscript((_, items) => compactEntries(items, chatEntries(items, 1))),
    minGroup10: byTranscript((_, items) => compactEntries(items, chatEntries(items, 10))),
  });
});

test('golden: chatEntries, edge sequences', () => {
  const tool = (name: string): TranscriptItem => ({ kind: 'tool', at: null, tool: { id: name, name, input: {} } });
  const think: TranscriptItem = { kind: 'thinking', at: null, text: 'hmm' };
  const text: TranscriptItem = { kind: 'text', at: null, text: 'hello' };
  const bare: TranscriptItem = { kind: 'tool', at: null };
  const seqs: Record<string, TranscriptItem[]> = {
    empty: [],
    'three tools': [tool('a'), tool('b'), tool('c')],
    'two tools': [tool('a'), tool('b')],
    'thinking first': [think, tool('a'), tool('b'), tool('c')],
    'thinking inside': [tool('a'), think, tool('b'), think, tool('c'), text],
    'thinking trailing short': [tool('a'), think],
    'tool without call': [tool('a'), bare, tool('b'), tool('c')],
  };
  checkGolden('transcript-model/chatEntries-edge', Object.fromEntries(Object.entries(seqs).map(([n, s]) => [n, compactEntries(s, chatEntries(s))])));
});

test('golden: visibleRange, constants', () => {
  checkGolden(
    'transcript-model/visibleRange',
    [
      [0, 150],
      [10, 150],
      [150, 150],
      [151, 150],
      [1000, 150],
      [5, 0],
      [5, -3],
    ].map(([total, count]) => [total, count, visibleRange(total, count)])
  );
  checkGolden('transcript-model/constants', { CHAT_PAGE, IDLE_AFTER_MS });
});

test('golden: per-transcript summaries (lastThinkingIndex, hasNativePrompt, transcriptStats, lastSeen)', () => {
  checkGolden(
    'transcript-model/lastThinkingIndex',
    byTranscript((_, items) => lastThinkingIndex(items))
  );
  checkGolden(
    'transcript-model/hasNativePrompt',
    byTranscript((_, items) => hasNativePrompt(items))
  );
  checkGolden(
    'transcript-model/transcriptStats',
    byTranscript((_, items) => transcriptStats(items))
  );
  checkGolden('transcript-model/lastSeen', {
    transcripts: byTranscript((t, items) => [lastSeen(items, t.execution.lastActivityAt), lastSeen(items, null), lastSeen(items, 9_999_999_999_999)]),
    empty: [lastSeen([], null), lastSeen([], undefined), lastSeen([], 5)],
  });
});

test('golden: synthesizedPrompt over every transcript', () => {
  const inputs = Object.fromEntries(transcripts.map(([n, t]) => [n, t.execution.input]));
  const refs = refsOf({ input: inputs });
  checkGolden(
    'transcript-model/synthesizedPrompt',
    {
      transcripts: byTranscript((t) => synthesizedPrompt(t.instructions, t.execution.input)),
      table: [
        [null, undefined],
        ['', null],
        ['  \n ', 'a string input'],
        ['Do it.', [1, 2]],
        ['Do it.', { short: 'x', long: 'y'.repeat(61), exactly60: 'z'.repeat(60), multi: 'a\nb', n: 5, nested: { a: 1 } }],
      ].map(([i, input]) => synthesizedPrompt(i as string | null, input as Json)),
    },
    refs
  );
});

test('golden: isNotice over every item kind', () => {
  const kinds = [...new Set(transcripts.flatMap(([, t]) => t.transcript.items.map((i) => i.kind))), 'system', 'auto', 'retry', 'rate_limit', 'compaction', 'text', 'error', 'steer', '', 'SYSTEM'];
  checkGolden('transcript-model/isNotice', Object.fromEntries([...new Set(kinds)].sort().map((k) => [k, isNotice(k)])));
});

test('golden: splitZeroshotPrompt and firstLine over every prompt and text', () => {
  const texts = transcripts.flatMap(([n, t]) => t.transcript.items.flatMap((item, i) => (typeof item.text === 'string' ? [[`${n}@${i}:${item.kind}`, item.text] as const] : [])));
  const prompts = texts.filter(([n]) => n.endsWith(':prompt'));
  const table = [
    '',
    'no frame at all',
    'Authored instructions:\nno graph-node head',
    'Execute this graph node.\nAuthored instructions:\n  Build it.  \n',
    'Execute this graph node.\nAuthored instructions:\nBuild it.\nRuntime-owned guidance\nInput JSON:\n{"a":1}\nResponse contract',
    'Execute this graph node.\nAuthored instructions:\nBuild it.\nInput JSON:\n{not json\nrest',
    'Execute this graph node.\nAuthored instructions:\nBuild it.\nInput JSON:\n',
    'Input JSON:\n{"early":true}\nExecute this graph node.\nAuthored instructions:\nX',
  ];
  checkGolden('transcript-model/splitZeroshotPrompt', { prompts: Object.fromEntries(prompts.map(([n, t]) => [n, splitZeroshotPrompt(t)])), table: table.map((t) => [t, splitZeroshotPrompt(t)]) });
  const lineTable = [null, undefined, '', '\n\n', '  # Heading  \nbody', '### x', '#nospace', `${'w'.repeat(130)}`, '\n  second line first'];
  checkGolden('transcript-model/firstLine', {
    texts: Object.fromEntries(texts.map(([n, t]) => [n, firstLine(t)])),
    table: lineTable.flatMap((t) => [firstLine(t), firstLine(t, 10)]),
  });
});

test('golden: commonDir and relativeTo over the tool paths of every transcript', () => {
  checkGolden('transcript-model/commonDir', {
    transcripts: byTranscript((_, items) => commonDir(toolPaths(items))),
    table: [
      [],
      ['/a/b/c.ts'],
      ['/a/b/c.ts', 'rel/x.ts'],
      ['/a/b/c.ts', '/a/b/d.ts'],
      ['/a/b/c/d.ts', '/a/b/c/e/f.ts'],
      ['/a/b', '/a/b'],
      ['/a/b/c', '/a/b/c/d'],
      ['/w/x/src/a.ts', '/w/xy/b.ts'],
      ['/a/x.ts', '/b/y.ts'],
    ].map((p) => [p, commonDir(p)]),
  });
  checkGolden('transcript-model/relativeTo', {
    transcripts: byTranscript((_, items) => {
      const paths = toolPaths(items);
      const dir = commonDir(paths);
      return paths.map((p) => relativeTo(p, dir));
    }),
    table: [
      ['/w/x/src/a.ts', '/w/x'],
      ['/w/x', '/w/x'],
      ['/w/x/', '/w/x'],
      ['/w/xy/a.ts', '/w/x'],
      ['src/a.ts', '/w/x'],
      ['/w/x/a', null],
      ['/w/x/a', ''],
    ].map(([p, w]) => [p, w, relativeTo(p as string, w)]),
  });
});
