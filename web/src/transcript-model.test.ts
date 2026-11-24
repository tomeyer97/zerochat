import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  chatEntries,
  deniedPermission,
  editPreview,
  exitNote,
  isNotice,
  isSummarisedTool,
  lastThinkingIndex,
  noticeClass,
  returnedJson,
  synthesizedPrompt,
  toolCardView,
  toolGroupSummary,
  toolPaths,
  toolSummary,
  transcriptStats,
  visibleRange,
} from './transcript-model';
import { diffLines } from './line-diff';
import type { TranscriptItem } from './types';

test('tool summaries name the action and its subject', () => {
  assert.deepEqual(toolSummary({ name: 'Bash', input: { command: 'npm test\nmore' } }), { verb: 'Bash', detail: 'npm test' });
  assert.deepEqual(toolSummary({ name: 'Edit', input: { file_path: 'src/x.ts' } }), { verb: 'Edit', detail: 'src/x.ts' });
  assert.deepEqual(toolSummary({ name: 'Grep', input: { pattern: 'foo', path: 'src' } }), { verb: 'Search', detail: 'foo  in  src' });
  assert.deepEqual(toolSummary({ name: 'mcp__x', input: { query: 'q' } }), { verb: 'mcp__x', detail: 'q' });
  assert.deepEqual(toolSummary({ name: 'Weird', input: null }), { verb: 'Weird', detail: '' });
});

test('edits preview as a real line diff, not old-then-new', () => {
  assert.equal(editPreview({ name: 'Edit', input: { file_path: 'a', old_string: 'a\nb', new_string: 'c' } }), '-a\n-b\n+c');
  // Shared lines (the 3-line docstring context around a one-line change) stay as context, not
  // deleted-then-re-added.
  assert.equal(
    editPreview({ name: 'Edit', input: { old_string: 'def f():\n    """doc"""\n    return a', new_string: 'def f():\n    """doc"""\n    return b' } }),
    ' def f():\n     """doc"""\n-    return a\n+    return b'
  );
  assert.equal(editPreview({ name: 'Bash', input: { command: 'ls' } }), null);
});

const tool = (name: string): TranscriptItem => ({ kind: 'tool', at: 1, tool: { id: name, name, input: {} } });
const text = (t: string): TranscriptItem => ({ kind: 'text', at: 1, text: t });
const thinking = (t: string): TranscriptItem => ({ kind: 'thinking', at: 1, text: t });

test('runs of 3+ tool calls fold into one group', () => {
  const entries = chatEntries([text('a'), tool('1'), tool('2'), tool('3'), text('b'), tool('4'), tool('5')]);
  assert.deepEqual(
    entries.map((e) => e.kind),
    ['item', 'tools', 'item', 'item', 'item']
  );
  assert.equal(entries[1].kind === 'tools' && entries[1].items.length, 3);
});

test('short reasoning between tool calls joins the group instead of breaking it', () => {
  const entries = chatEntries([text('a'), tool('1'), thinking('hm'), tool('2'), thinking('ok'), tool('3')]);
  assert.deepEqual(
    entries.map((e) => e.kind),
    ['item', 'tools']
  );
  // All 5 items (3 tools + 2 thoughts) stay in the group, in order, for when it opens.
  assert.equal(entries[1].kind === 'tools' && entries[1].items.length, 5);
  // A run too small to fold still renders its reasoning, not just its tools.
  const small = chatEntries([tool('1'), thinking('hm'), tool('2')]);
  assert.deepEqual(
    small.map((e) => e.kind),
    ['item', 'item', 'item']
  );
  // A thought before any tool call has nothing to join, so it stands alone as usual.
  const lone = chatEntries([thinking('hm'), tool('1'), tool('2'), tool('3')]);
  assert.deepEqual(
    lone.map((e) => e.kind),
    ['item', 'tools']
  );
});

test('windowing keeps the latest items', () => {
  assert.deepEqual(visibleRange(1000, 150), { start: 850, end: 1000 });
  assert.deepEqual(visibleRange(10, 150), { start: 0, end: 10 });
});

test('prompt synthesis separates long text fields from the rest of the input', () => {
  const p = synthesizedPrompt('Do it', { task: 'x'.repeat(100), ledger: 'a\nb', issueNumber: '12', n: 3 });
  assert.equal(p.instructions, 'Do it');
  assert.deepEqual(
    p.textFields.map((f) => f.name),
    ['task', 'ledger']
  );
  assert.deepEqual(p.otherFields, { issueNumber: '12', n: 3 });
  assert.equal(synthesizedPrompt('  ', null).instructions, null);
});

test('stats and the latest reasoning', () => {
  const items: TranscriptItem[] = [
    { kind: 'thinking', at: 1, text: 'a' },
    { kind: 'tool', at: 2, tool: { id: '1', name: 'Bash', input: {}, result: { isError: true, content: 'boom', at: 3 } } },
    { kind: 'thinking', at: 4, text: 'b' },
    text('done'),
  ];
  assert.equal(lastThinkingIndex(items), 2);
  assert.deepEqual(transcriptStats(items), { tools: 1, toolErrors: 1, messages: 1, thinking: 2 });
});

test('Copilot tools read like their Claude counterparts', () => {
  assert.deepEqual(toolSummary({ name: 'bash', input: { command: 'git status\nx', description: 'Inspect' } }), { verb: 'Bash', detail: 'git status' });
  assert.deepEqual(toolSummary({ name: 'view', input: { path: '/w/a.md', view_range: [1, 9] } }), { verb: 'Read', detail: '/w/a.md' });
  assert.deepEqual(toolSummary({ name: 'read_bash', input: { shellId: 'yarn', delay: 5 } }), { verb: 'Read shell', detail: 'yarn' });
  assert.equal(editPreview({ name: 'edit', input: { path: 'a', old_str: 'a', new_str: 'b' } }), '-a\n+b');
  assert.equal(editPreview({ name: 'create', input: { path: 'a', file_text: 'x\ny' } }), '+x\n+y');
});

test('exit codes and refused permissions are flagged, approvals and clean exits are not', () => {
  assert.equal(exitNote({ isError: false, exitCode: 2 }), 'exit 2');
  assert.equal(exitNote({ isError: false, exitCode: 0 }), null);
  assert.equal(exitNote({ isError: true, exitCode: 1 }), null);
  assert.equal(exitNote(undefined), null);
  assert.equal(deniedPermission({ kind: 'shell', result: 'approved' }), null);
  assert.equal(deniedPermission({ kind: 'shell', result: 'denied-by-rules' }), 'shell permission denied by rules');
  assert.equal(deniedPermission({ kind: 'read' }), null);
  assert.ok(isNotice('compaction'));
});

test('agent text that is only a JSON object is a returned result, fence stripped', () => {
  assert.equal(returnedJson('{"verdict": "accepted"}'), '{"verdict": "accepted"}');
  assert.equal(returnedJson('  ```json\n{"a": 1}\n```  '), '{"a": 1}');
  assert.equal(returnedJson('{not json'), null);
  assert.equal(returnedJson('Done. {"a": 1}'), null);
  assert.equal(returnedJson('[1, 2]'), null);
});

test('a tool group head counts calls, failures and verbs, skipping reasoning in between', () => {
  const tool = (name: string, isError = false): TranscriptItem => ({ kind: 'tool', at: null, tool: { id: name, name, input: {}, result: { content: '', isError, at: null } } as never });
  const items = [tool('Read'), { kind: 'thinking', at: null, text: 'hmm' } as TranscriptItem, tool('Read', true), tool('Edit'), tool('view')].map((item) => ({ item }));
  assert.deepEqual(toolGroupSummary(items), { calls: 4, errors: 1, verbs: 'Read ×3, Edit ×1' });
});

test('a tool card shows paths inside the workspace, and folds the input when the summary says it', () => {
  const item: TranscriptItem = { kind: 'tool', at: null, tool: { id: '1', name: 'Grep', input: { pattern: 'TODO', path: '/w/x/src', description: 'find todos' } } };
  const card = toolCardView(item, item.tool!, true, '/w/x');
  assert.equal(card.status, 'pending');
  assert.equal(card.verb, 'Search');
  assert.equal(card.detail, 'TODO  in  src');
  assert.equal(card.title, 'TODO  in  /w/x/src\nfind todos');
  assert.equal(card.foldInput, true);
  assert.equal(toolCardView(item, item.tool!, false, null).status, 'none');
  const result: TranscriptItem = { kind: 'tool', at: null, tool: { id: '2', name: 'StructuredOutput', input: {}, permission: { kind: 'shell', result: 'approved-for-session' } } };
  const resultCard = toolCardView(result, result.tool!, false, null);
  assert.deepEqual([resultCard.resultTool, resultCard.verb, resultCard.detail, resultCard.title], [true, 'Returned result', '', 'StructuredOutput']);
  assert.equal(resultCard.permission, 'shell permission approved for session');
  assert.equal(resultCard.denied, null);
});

test('which tools fold their input: the summarised ones only', () => {
  for (const name of ['Bash', 'Read', 'Edit', 'MultiEdit', 'Write', 'Glob', 'Grep', 'WebFetch', 'WebSearch', 'NotebookEdit', 'bash', 'view', 'edit', 'create', 'grep', 'glob', 'web_fetch']) {
    assert.equal(isSummarisedTool(name), true, name);
  }
  for (const name of ['Task', 'Agent', 'TodoWrite', 'read_bash', 'write_bash', 'stop_bash', 'str_replace', 'rg', 'task', 'StructuredOutput', 'mcp__x', 'constructor', '']) {
    assert.equal(isSummarisedTool(name), false, name);
  }
});

test('tool paths: the file-naming keys of an object input', () => {
  assert.deepEqual(toolPaths({ input: { file_path: '/a', path: '/b', notebook_path: '/c', url: '/d' } }), ['/a', '/b', '/c']);
  assert.deepEqual(toolPaths({ input: { path: 3 } }), []);
  assert.deepEqual(toolPaths({ input: ['/a'] }), []);
  assert.deepEqual(toolPaths(undefined), []);
});

test('notice kinds and their CSS classes', () => {
  assert.equal(isNotice('rate_limit'), true);
  assert.equal(isNotice('constructor'), false);
  assert.equal(noticeClass('rate_limit'), 'notice-rate_limit');
  assert.equal(noticeClass('auto'), 'notice-auto');
});

test('a line diff keeps shared lines and falls back to old-then-new above its size limit', () => {
  assert.deepEqual(diffLines('a\nb\nc', 'a\nB\nc'), [' a', '-b', '+B', ' c']);
  assert.deepEqual(diffLines('a\nb', 'a\nb', 1), ['-a', '-b', '+a', '+b']);
});
