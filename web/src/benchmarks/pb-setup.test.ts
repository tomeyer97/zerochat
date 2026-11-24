import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { ApiError } from '../api';
import { PB_NOT_CONFIGURED, isPbNotConfigured, notePbAnswer, pbGateState, pbSaidNotConfigured } from './pb-setup';
import { PbGateView } from './PbGate';
import { BenchmarksNav } from './shared';

const nav = (...ids: string[]) => ({ targets: ids.map((id) => ({ id })) }) as never;

test('pb is configured exactly when the navigation has the pb target', () => {
  assert.equal(pbGateState(undefined, false, false), 'checking');
  assert.equal(pbGateState(nav('local'), false, false), 'missing');
  assert.equal(pbGateState(nav('local', 'pb'), false, false), 'ready');
  // The navigation could not be read: the page's own request says what is wrong.
  assert.equal(pbGateState(undefined, true, false), 'ready');
  // A benchmarks route answered pb_not_configured: not configured, whatever the nav said.
  assert.equal(pbGateState(nav('pb'), false, true), 'missing');
});

test('the setup check saying pb is not configured wins, and the page waits for it (F22)', () => {
  assert.equal(pbGateState(nav('local', 'pb'), false, false, { configured: false }), 'missing');
  assert.equal(pbGateState(nav('local', 'pb'), false, false, 'loading'), 'checking', 'no benchmarks request before /api/setup answers');
  assert.equal(pbGateState(nav('local', 'pb'), false, false, { configured: true }), 'ready');
  assert.equal(pbGateState(nav('local'), false, false, 'loading'), 'missing');
  assert.equal(pbGateState(undefined, true, false, null), 'ready');
});

test('the 404 pb_not_configured answer is "not configured", any other failure is not', () => {
  assert.equal(isPbNotConfigured(new ApiError(PB_NOT_CONFIGURED, 'm', 404)), true);
  assert.equal(isPbNotConfigured(new ApiError('batch_not_found', 'm', 404)), false);
  assert.equal(isPbNotConfigured(new Error('m')), false);
  notePbAnswer(new ApiError('pb_failed', 'm', 400));
  assert.equal(pbSaidNotConfigured(), false);
  notePbAnswer(new ApiError(PB_NOT_CONFIGURED, 'm', 404));
  assert.equal(pbSaidNotConfigured(), true);
});

test('without pb a Benchmarks page is one neutral page: no tabs, no Start, no Retry', () => {
  const page = createElement(BenchmarksNav, { active: 'batches' });
  const html = renderToStaticMarkup(createElement(PbGateView, { state: 'missing' }, page));
  assert.match(html, /Benchmarks needs pb \(ProgramBench runner\), which is not configured here\./);
  assert.match(html, /<code>pb_root<\/code>/);
  for (const absent of ['Batches', 'Sweeps', 'Start a batch', 'Retry', 'bm-subnav']) assert.ok(!html.includes(absent), absent);
  const ready = renderToStaticMarkup(createElement(PbGateView, { state: 'ready' }, page));
  assert.match(ready, /Start a batch/);
});
