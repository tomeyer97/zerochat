// Goldens for diff.ts over every diff, tree and change list the API snapshots hold
// (`api/workspace/**`, `api/features/**`), plus tables of tricky inputs.
import { apiBodies, checkGolden } from './testdata/golden';
import { test } from 'node:test';
import { buildTree, fileStatusLabel, isArtefact, parseDiff } from './diff';
import type { ChangesResponse, FeatureChanges, SliceCommit, SliceTask, WorktreeDiff } from './types';

const DIFF_TABLE = [
  '',
  '\n',
  'not a diff at all',
  'diff --git a/x.ts b/x.ts\nindex 5d21c42..0d25a22 100644\n--- a/x.ts\n+++ b/x.ts\n@@ -1,3 +1,4 @@ function f() {\n ctx\n-old\n+new\n+added\n ctx2\n',
  'diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n\\ No newline at end of file',
  'diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n',
  'diff --git a/old name.txt b/new name.txt\nsimilarity index 90%\nrename from old name.txt\nrename to new name.txt\n',
  'diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n',
  '--- a/headless.txt\n+++ b/headless.txt\n@@ -1 +1 @@\n-a\n+b\n',
  '@@ -5,2 +7,2 @@\n x\n-y\n+z\n',
  'diff --git a/a b/a\n@@ -1,2 +1,2 @@\n a\n\n-b\n+c\n… [diff truncated: 400 more lines]\n',
  'diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n---- a line that starts with dashes\n++++ and pluses\n',
  'diff --git a/a b/a\n@@ malformed hunk @@\n+x\n',
  'diff --git weird header\n--- a/p\n+++ b/q\n@@ -1 +1 @@\n-1\n+2',
  'diff --git a/one b/one\n--- a/one\n+++ b/one\n@@ -1 +1 @@\n-1\n+2\ndiff --git a/two b/two\n--- a/two\n+++ b/two\n@@ -10,0 +11,2 @@\n+3\n+4\n',
];
const PATHS = [
  'README.md',
  'src/a.ts',
  'src/b.ts',
  'src/lib/c.ts',
  'src',
  'src/a.ts',
  'Zeta.txt',
  'alpha.txt',
  'é.txt',
  'e.txt',
  'node_modules/x/index.js',
  'a/node_modules/y.js',
  'my_node_modules/z.js',
  '__pycache__/m.cpython-312.pyc',
  'pkg/__pycache__/m.pyc',
  'm.pyc',
  'm.pyo',
  'tsconfig.tsbuildinfo',
  '.DS_Store',
  'dir/.DS_Store',
  'x.DS_Store',
  '.venv/bin/python',
  '.pytest_cache/v',
  '.mypy_cache/x',
  '.ruff_cache/x',
  '.tox/x',
  'pkg.egg-info/PKG-INFO',
  'egg-info',
  '',
  '/abs/path.ts',
  'trailing/',
  'double//slash.ts',
];

/** Every diff text the snapshots hold. */
function diffs(): [string, string][] {
  return [
    ...apiBodies<{ diff: string }>('workspace', /diff[^/]*\.json$/).map(([n, d]) => [`workspace/${n}`, d.diff] as [string, string]),
    ...apiBodies<FeatureChanges>('features', /\/changes(-diff)?\.json$/).flatMap(([n, c]) => (c.diff != null ? [[`features/${n}`, c.diff] as [string, string]] : [])),
    ...apiBodies<SliceCommit>('features', /\/commit-[^/]*\.json$/).map(([n, c]) => [`features/${n}`, c.diff] as [string, string]),
    ...apiBodies<SliceTask>('features', /\/task-[^/]*\.json$/).flatMap(([n, t]) => (t.diff != null ? [[`features/${n}`, t.diff] as [string, string]] : [])),
    ...apiBodies<WorktreeDiff>('features', /\/worktree-[^/]*\.json$/).map(([n, w]) => [`features/${n}`, w.diff] as [string, string]),
  ];
}

/** Every path list the snapshots hold: file trees and changed files. */
function pathLists(): [string, string[]][] {
  return [
    ...apiBodies<{ paths: string[] }>('workspace', /tree[^/]*\.json$/).map(([n, t]) => [`workspace/${n}`, t.paths] as [string, string[]]),
    ...apiBodies<ChangesResponse>('workspace', /changes[^/]*\.json$/).map(([n, c]) => [`workspace/${n}`, c.files.map((f) => f.path)] as [string, string[]]),
    ...apiBodies<FeatureChanges>('features', /\/changes(-diff)?\.json$/).map(([n, c]) => [`features/${n}`, c.stat.files.map((f) => f.path)] as [string, string[]]),
  ];
}

test('golden: parseDiff over every diff in the snapshots', () => {
  checkGolden('diff/parseDiff-snapshots', Object.fromEntries(diffs().map(([n, d]) => [n, parseDiff(d)])));
});

test('golden: parseDiff over a table of tricky diffs', () => {
  checkGolden(
    'diff/parseDiff-table',
    DIFF_TABLE.map((d) => [d, parseDiff(d)])
  );
});

test('golden: buildTree over every path list in the snapshots, and a table', () => {
  checkGolden('diff/buildTree', {
    snapshots: Object.fromEntries(pathLists().map(([n, p]) => [n, buildTree(p)])),
    table: buildTree(PATHS),
    empty: buildTree([]),
  });
});

test('golden: isArtefact over every path in the snapshots, and a table', () => {
  const all = [...new Set([...pathLists().flatMap(([, p]) => p), ...PATHS])].sort();
  checkGolden('diff/isArtefact', Object.fromEntries(all.map((p) => [p, isArtefact(p)])));
});

test('golden: fileStatusLabel, with every change status in the snapshots', () => {
  const statuses = [...new Set(apiBodies<ChangesResponse>('workspace', /changes[^/]*\.json$/).flatMap(([, c]) => c.files.map((f) => f.status)))].sort();
  checkGolden('diff/fileStatusLabel', { fileStatusLabel, snapshots: Object.fromEntries(statuses.map((s) => [s, fileStatusLabel[s] ?? null])) });
});
