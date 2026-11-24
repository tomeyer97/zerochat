// The change marks of a live Files list (DECISIONS D5): what moved between two loads, and where a
// removed file is drawn while it fades.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileRows, listChanges, NO_CHANGES } from './file-changes';
import type { ChangedFile } from './types';

const file = (path: string, additions = 1, deletions = 0, status = 'M'): ChangedFile => ({ path, status, additions, deletions });
const paths = (set: ReadonlySet<string>) => [...set].sort();

test('the first load marks nothing', () => {
  assert.equal(listChanges(undefined, [file('a')]), NO_CHANGES);
  assert.deepEqual(
    fileRows(undefined, [file('a')], NO_CHANGES).map((r) => r.mark),
    [null]
  );
});

test('an unchanged reload marks nothing', () => {
  const changes = listChanges([file('a'), file('b', 3, 2)], [file('a'), file('b', 3, 2)]);
  assert.deepEqual([paths(changes.added), paths(changes.changed), changes.removed], [[], [], []]);
});

test('a new path is added, moved counts or status are changed, a missing path is removed', () => {
  const before = [file('a'), file('b', 3, 2), file('c'), file('d', 1, 0, 'A')];
  const after = [file('a'), file('b', 4, 2), file('d', 1, 0, 'M'), file('e')];
  const changes = listChanges(before, after);
  assert.deepEqual(paths(changes.added), ['e']);
  assert.deepEqual(paths(changes.changed), ['b', 'd']);
  assert.deepEqual(
    changes.removed.map((f) => f.path),
    ['c']
  );
});

test('binary and untracked flags count as a change; null counts equal zero', () => {
  const before: ChangedFile[] = [{ path: 'x', status: 'M', additions: null, deletions: null }, file('y')];
  const after: ChangedFile[] = [
    { path: 'x', status: 'M', additions: 0, deletions: 0 },
    { ...file('y'), untracked: true },
  ];
  assert.deepEqual(paths(listChanges(before, after).changed), ['y']);
});

test('rows carry each file’s mark', () => {
  const before = [file('a'), file('b')];
  const after = [file('a', 2), file('b'), file('c')];
  const rows = fileRows(before, after, listChanges(before, after));
  assert.deepEqual(
    rows.map((r) => [r.file.path, r.mark]),
    [
      ['a', 'changed'],
      ['b', null],
      ['c', 'added'],
    ]
  );
});

test('a removed file is drawn where it stood: after the nearest earlier file still listed', () => {
  const before = [file('first'), file('a'), file('gone1'), file('gone2'), file('b'), file('last')];
  const after = [file('a'), file('new'), file('b')];
  const rows = fileRows(before, after, listChanges(before, after));
  assert.deepEqual(
    rows.map((r) => `${r.file.path}${r.mark ? `:${r.mark}` : ''}`),
    ['first:removed', 'a', 'gone1:removed', 'gone2:removed', 'new:added', 'b', 'last:removed']
  );
});

test('when every file is removed, all of them are drawn fading in their order', () => {
  const before = [file('a'), file('b')];
  const rows = fileRows(before, [], listChanges(before, []));
  assert.deepEqual(
    rows.map((r) => [r.file.path, r.mark]),
    [
      ['a', 'removed'],
      ['b', 'removed'],
    ]
  );
});
