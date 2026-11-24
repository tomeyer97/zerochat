// Golden for the Batches list's row text (benchmarks-model/batchRow) over every batch in the API
// snapshot (`api/benchmarks/batches.json`), plus a table of unscored statuses.
import { test } from 'node:test';
import { FIXED_NOW, apiFile, checkGolden } from '../testdata/golden';
import type { BatchSummary } from '../types';
import { batchCompactText, batchDoneTitle, batchIdentity, batchNoCostReason, batchNoScoreReason, batchScoreText } from './batch-list';

const summaries = apiFile<{ batches: BatchSummary[] }>('benchmarks/batches.json').body.batches;

test('golden: batch list rows (identity, score, why "—", compact line)', () => {
  const row = (b: BatchSummary) => ({
    identity: batchIdentity(b),
    score: batchScoreText(b),
    noScore: batchNoScoreReason(b),
    noCost: batchNoCostReason(b),
    doneTitle: batchDoneTitle(b),
    compact: batchCompactText(b, FIXED_NOW),
  });
  const base = summaries.find((b) => b.status === 'done' && b.meanScore != null)!;
  const unscored = { ...base, meanScore: null, minScore: null, maxScore: null, totalCostUsd: null };
  const table = Object.fromEntries(
    (['stopped', 'crashed', 'failed_to_start', 'running', 'starting', 'done'] as const).map((status) => [status, row({ ...unscored, status, runsDone: status === 'running' ? 0 : 1 })])
  );
  checkGolden('benchmarks-model/batchRow', {
    batches: Object.fromEntries(summaries.map((b) => [b.id, row(b)])),
    unscored: table,
    oneTaskOneSeed: row({ ...base, taskCount: 1, seedCount: 1, retriedAttempts: 2 }),
  });
});
