import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CELL_STATES, STATE_LEGEND, cellGroup, countByGroup, summaryText, type CellGroup } from './cell-states';

test('the legend names every group but scored exactly once, in the class the matrix draws it', () => {
  const legendGroups = STATE_LEGEND.flatMap((item) => item.groups);
  const groups = new Set(Object.values(CELL_STATES).map((row) => row.group));
  groups.delete('scored');
  assert.deepEqual([...legendGroups].sort(), [...groups].sort());
  // A coloured group's legend item uses the tone its states have in the matrix.
  for (const item of STATE_LEGEND) {
    for (const [state, row] of Object.entries(CELL_STATES)) {
      if (!item.groups.includes(row.group) || !row.tone || row.tone === 'muted') continue;
      assert.equal(item.className, `tone-text-${row.tone}`, `${state} is drawn ${row.tone}`);
    }
  }
});

test('cells are counted per group; a state a newer pb added counts in none', () => {
  const counts = countByGroup([{ state: 'failed' }, { state: 'crashed' }, { state: 'evaluating' }, { state: 'over_steps' }, { state: 'from_the_future' }]);
  assert.deepEqual(counts, { scored: 0, running: 1, queued: 0, failed: 2, limit: 1, interrupted: 0, notStarted: 0 });
  assert.equal(cellGroup('from_the_future'), null);
});

test('the summary names only the groups that have cells, in a fixed order', () => {
  const none: Record<CellGroup, number> = { scored: 2, running: 0, queued: 0, failed: 0, limit: 0, interrupted: 0, notStarted: 0 };
  assert.equal(summaryText(none, 4, 0), '2/4 scored');
  const some = { ...none, running: 1, failed: 1, limit: 2, notStarted: 1 };
  assert.equal(summaryText(some, 6, 1), '2/6 scored (1 cut short, in the mean), 1 running, 1 failed, 2 over a limit, 1 not started');
});
