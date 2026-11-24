// The Files list on a live run: a refresh keeps the rows on screen (no skeleton), only the very
// first load shows one; an open diff keeps its hidden line labels inside its own scroll container.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { ChangesBody, changesKey, DiffView } from './Files';
import type { Resource } from './hooks';
import type { ChangeRange } from './diff';
import type { ChangesResponse, RunView } from './types';

const view = { target: 'local', runId: 'r1', cursor: 7 } as RunView;
const range: ChangeRange = { from: 'a'.repeat(40), to: null, perExecution: true };
const data: ChangesResponse = {
  files: [
    { path: 'src/one.ts', status: 'M', additions: 3, deletions: 1 },
    { path: 'src/two.ts', status: 'A', additions: 9, deletions: 0 },
  ],
};
const resource = (state: Partial<Resource<ChangesResponse>>): Resource<ChangesResponse> => ({
  data: undefined,
  error: undefined,
  errorStatus: undefined,
  loading: false,
  reloading: false,
  reload: () => {},
  ...state,
});
const render = (changes: Resource<ChangesResponse>) => renderToStaticMarkup(createElement(ChangesBody, { view, range, changes, refresh: 7 }));
const SKELETON = 'aria-busy="true"';

test('a run event (a new cursor) or a new end snapshot does not make it a different list', () => {
  assert.equal(changesKey({ ...view, cursor: 8 } as RunView, range), changesKey(view, range));
  const ended: ChangeRange = { ...range, to: 'b'.repeat(40) };
  assert.equal(changesKey(view, ended), changesKey(view, range));
  assert.notEqual(changesKey(view, { ...range, from: 'c'.repeat(40) }), changesKey(view, range));
});

test('the first load shows a skeleton', () => {
  assert.ok(render(resource({ loading: true })).includes(SKELETON));
});

test('a refresh keeps the rows on screen, with no skeleton or spinner', () => {
  const shown = render(resource({ data }));
  const refreshing = render(resource({ data, reloading: true }));
  assert.equal(refreshing, shown);
  assert.ok(!refreshing.includes(SKELETON));
  assert.match(refreshing, /src\/one\.ts[\s\S]*src\/two\.ts/);
});

test('a failed refresh keeps the rows too', () => {
  const html = render(resource({ data, error: 'The server is busy.', errorStatus: 503 }));
  assert.match(html, /src\/one\.ts/);
  assert.ok(!html.includes('not available'));
});

test('a diff’s hidden line labels are inside the diff, its own containing block', () => {
  const html = renderToStaticMarkup(createElement(DiffView, { text: 'diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n' }));
  assert.match(html, /^<div class="diff">[\s\S]*<span class="visually-hidden">added: <\/span>[\s\S]*<\/div>$/);
});
