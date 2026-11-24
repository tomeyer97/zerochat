import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildTree, isArtefact, parseDiff } from './diff';

const sample = `diff --git a/src/a.ts b/src/a.ts
index 1..2 100644
--- a/src/a.ts
+++ b/src/a.ts
@@ -10,3 +10,4 @@ function x() {
 keep
-old
--- not a header
+new
+added
diff --git a/b.txt b/b.txt
new file mode 100644
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+hello
`;

test('parses files, hunks and line numbers', () => {
  const files = parseDiff(sample);
  assert.equal(files.length, 2);
  const [a, b] = files;
  assert.equal(a.newPath, 'src/a.ts');
  assert.equal(a.additions, 2);
  assert.equal(a.deletions, 2);
  const lines = a.lines.filter((l) => l.kind !== 'meta');
  assert.deepEqual(
    lines.map((l) => [l.kind, l.oldNo, l.newNo]),
    [
      ['hunk', undefined, undefined],
      ['ctx', 10, 10],
      ['del', 11, undefined],
      ['del', 12, undefined],
      ['add', undefined, 11],
      ['add', undefined, 12],
    ]
  );
  assert.equal(b.newPath, 'b.txt');
  assert.equal(b.additions, 1);
});

test('an empty context line inside a hunk keeps the numbering', () => {
  const [f] = parseDiff('--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n-b\n+c\n');
  assert.deepEqual(
    f.lines.map((l) => [l.kind, l.oldNo, l.newNo]),
    [
      ['hunk', undefined, undefined],
      ['ctx', 1, 1],
      ['ctx', 2, 2],
      ['del', 3, undefined],
      ['add', undefined, 3],
    ]
  );
});

test('the server synthetic untracked diff has no diff --git header', () => {
  const files = parseDiff('--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n');
  assert.equal(files.length, 1);
  assert.equal(files[0].newPath, 'new.txt');
  assert.equal(files[0].additions, 2);
});

test('folder tree, folders first', () => {
  const tree = buildTree(['b.txt', 'src/z.ts', 'src/a/x.ts', 'a.txt']);
  assert.deepEqual(
    tree.map((e) => e.name),
    ['src', 'a.txt', 'b.txt']
  );
  assert.deepEqual(
    tree[0].children!.map((e) => e.name),
    ['a', 'z.ts']
  );
});

test('the index line is dropped; generated files are recognised', () => {
  assert.ok(!parseDiff(sample)[0].lines.some((l) => l.text.startsWith('index ')));
  assert.ok(isArtefact('tests/__pycache__/test_core.cpython-312.pyc'));
  assert.ok(isArtefact('web/node_modules/x/index.js'));
  assert.ok(isArtefact('pkg.egg-info/PKG-INFO'));
  assert.ok(!isArtefact('src/pycache.ts'));
  assert.ok(!isArtefact('textkit/core.py'));
});
