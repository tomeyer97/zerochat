import { test } from 'node:test';
import assert from 'node:assert/strict';
import { BARS_MIN_WIDTH, gridTemplate, rowMatches, stateTone, timelineLayout, TIMELINE_COLUMNS, TOKENS_MIN_WIDTH, visibleColumns } from './timeline-model';
import type { RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };

const view = reviewFailed as unknown as RunView;

test('columns: every one on a wide table; bars, then tokens, give way as it narrows', () => {
  const wide = visibleColumns(timelineLayout(view, 1000)).map((c) => c.id);
  assert.deepEqual(
    wide,
    TIMELINE_COLUMNS.map((c) => c.id)
  );
  const noBars = visibleColumns(timelineLayout(view, BARS_MIN_WIDTH - 1)).map((c) => c.id);
  assert.ok(!noBars.includes('bar') && noBars.includes('tokens'));
  const narrow = visibleColumns(timelineLayout(view, TOKENS_MIN_WIDTH - 1)).map((c) => c.id);
  assert.ok(!narrow.includes('tokens'));
  assert.equal(gridTemplate(visibleColumns(timelineLayout(view, 1000))), 'minmax(90px, 1.2fr) 44px 80px 64px 56px 58px minmax(0, 1.6fr) minmax(80px, 1.2fr)');
});

test('columns that would be empty are not drawn', () => {
  const single = { ...view, executions: view.executions.slice(0, 1).map((e) => ({ ...e, round: 1, attempt: 1, state: 'accepted' })) } as unknown as RunView;
  const layout = timelineLayout({ ...single, failure: null }, 1000);
  assert.equal(layout.rounds, false);
  assert.equal(layout.bars, false, 'one execution needs no bar');
});

test('the filter matches every word against what the row shows', () => {
  const culprit = view.executions.find((e) => e.id === 26)!;
  assert.equal(rowMatches(culprit, view, ''), true);
  assert.equal(rowMatches(culprit, view, 'code usage'), true, 'the note text');
  assert.equal(rowMatches(culprit, view, 'code nothing-like-this'), false);
});

test('state tones: red only for a failure, amber for a rejected culprit', () => {
  assert.equal(stateTone('failed', false), 'bad');
  assert.equal(stateTone('rejected', true), 'warn');
  assert.equal(stateTone('rejected', false), 'muted');
  assert.equal(stateTone('running', false), 'running');
});
