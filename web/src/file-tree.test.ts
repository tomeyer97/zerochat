import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isArtefact } from './file-tree';

test('artefact folders count anywhere in the path, but only as folders', () => {
  for (const path of ['.venv/lib/x.py', 'a/.tox/log', 'a/.ruff_cache/x', 'a/.mypy_cache/x', '.pytest_cache/v', 'node_modules/', 'mypkg.egg-info/PKG-INFO', 'src/.egg-info/x']) {
    assert.ok(isArtefact(path), path);
  }
  for (const path of ['node_modules', 'a/__pycache__', 'mypkg.egg-info', 'my.venv/x', 'egg-info/x', 'src/node_modules.ts']) {
    assert.ok(!isArtefact(path), path);
  }
});

test('artefact files by name or extension', () => {
  for (const path of ['.DS_Store', 'a/b/.DS_Store', 'x.pyc', 'a/x.pyo', 'tsconfig.tsbuildinfo']) assert.ok(isArtefact(path), path);
  for (const path of ['x.DS_Store', '.DS_Store/x', 'x.pyc.txt', 'pyc', 'x.pyc/']) assert.ok(!isArtefact(path), path);
});
