// Goldens for features-model.ts over every feature the API snapshots hold (`api/features/**`:
// the list, each detail, events, changes, commits, tasks, worktrees and artifacts).
import { FIXED_NOW, apiBodies, apiFile, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import ELK from 'elkjs/lib/elk.bundled.js';
import {
  SLICE_CARD,
  attemptDuration,
  attemptKindLabel,
  attemptResult,
  buildSliceElk,
  cleanOutput,
  eventSlices,
  eventText,
  eventTone,
  failureWord,
  featureState,
  isActiveSlice,
  isNoiseEvent,
  nowParts,
  oneLine,
  parseDeps,
  pausedSummary,
  progressText,
  reasonSummary,
  roundedPath,
  shortDuration,
  sliceDuration,
  sliceEdges,
  sliceGraphKey,
  sliceStatusLabel,
  sliceTone,
  sortFeatures,
  specBody,
  splitDiff,
  statText,
  verdictLine,
  waitVerb,
  waitingText,
} from './features-model';
import type { FeatureChanges, FeatureDetail, FeatureEvent, FeatureRow, SliceCommit, SliceStatus, SliceTask, SliceView, WorktreeDiff } from './types';

const list = apiFile<{ features: FeatureRow[]; now: number }>('features/list.json').body;
const details = apiBodies<FeatureDetail>('features', /\/detail\.json$/).map(([n, d]) => [n.replace(/\/detail$/, ''), d] as [string, FeatureDetail]);
const events = apiBodies<{ events: FeatureEvent[] }>('features', /\/events(-[^/]*)?\.json$/);
const allEvents: [string, FeatureEvent][] = events.flatMap(([n, b]) => b.events.map((e, i) => [`${n}#${i}`, e] as [string, FeatureEvent]));
const slices: [string, SliceView, FeatureDetail][] = details.flatMap(([n, d]) => d.slices.map((s) => [`${n}/${s.id}`, s, d] as [string, SliceView, FeatureDetail]));
const rows: [string, FeatureRow][] = [...list.features.map((r) => [`list/${r.name}`, r] as [string, FeatureRow]), ...details.map(([n, d]) => [`detail/${n}`, d] as [string, FeatureRow])];
const bySlice = <T>(f: (s: SliceView, d: FeatureDetail) => T) => Object.fromEntries(slices.map(([n, s, d]) => [n, f(s, d)]));
const byRow = <T>(f: (r: FeatureRow) => T) => Object.fromEntries(rows.map(([n, r]) => [n, f(r)]));
const STATUSES: SliceStatus[] = ['pending', 'launching', 'running', 'integrating', 'merged', 'waiting_for_approval', 'failed', 'interrupted', 'skipped', 'novel' as SliceStatus];

/** Every diff text the feature snapshots hold. */
const diffs: [string, string][] = [
  ...apiBodies<FeatureChanges>('features', /\/changes(-diff)?\.json$/).flatMap(([n, c]) => (c.diff != null ? [[n, c.diff] as [string, string]] : [])),
  ...apiBodies<SliceCommit>('features', /\/commit-[^/]*\.json$/).map(([n, c]) => [n, c.diff] as [string, string]),
  ...apiBodies<SliceTask>('features', /\/task-[^/]*\.json$/).flatMap(([n, t]) => (t.diff != null ? [[n, t.diff] as [string, string]] : [])),
  ...apiBodies<WorktreeDiff>('features', /\/worktree-[^/]*\.json$/).map(([n, w]) => [n, w.diff] as [string, string]),
];
/** Every free text a feature carries (reasons, notes, task texts, artifacts, event details). */
const texts: [string, string][] = [
  ...slices.flatMap(
    ([n, s]) =>
      [
        ...(s.reason ? [[`${n}:reason`, s.reason]] : []),
        ...s.attempts.flatMap((a) => (a.reason ? [[`${n}#${a.n}:reason`, a.reason]] : [])),
        ...s.notes.map((t, i) => [`${n}:note${i}`, t]),
        ...s.autoNotes.map((t, i) => [`${n}:autoNote${i}`, t.text]),
      ] as [string, string][]
  ),
  ...apiBodies<SliceTask>('features', /\/task-[^/]*\.json$/).map(([n, t]) => [n, t.text] as [string, string]),
  ...apiBodies<{ content: string }>('features', /\/artifact-[^/]*\.json$/).map(([n, a]) => [n, a.content] as [string, string]),
  ...allEvents.flatMap(([n, e]) => (e.detail ? [[n, e.detail] as [string, string]] : [])),
];
const TEXT_TABLE = [null, undefined, '', '\n\n', 'one', '  a  \n----\n\n b \n====\n***\n', 'tests failed\n----------\n3 passed, 1 failed\n', '__\n--', 'x\n  ----  \n- - -\ny'];

test('golden: slice status tables (sliceTone, sliceStatusLabel, isActiveSlice)', () => {
  checkGolden('features-model/sliceTone', { table: Object.fromEntries(STATUSES.map((s) => [s, sliceTone(s)])), slices: bySlice((s) => sliceTone(s.status)) });
  checkGolden('features-model/sliceStatusLabel', { table: Object.fromEntries(STATUSES.map((s) => [s, sliceStatusLabel(s)])), slices: bySlice((s) => sliceStatusLabel(s.status)) });
  checkGolden('features-model/isActiveSlice', { table: Object.fromEntries(STATUSES.map((s) => [s, isActiveSlice(s)])), slices: bySlice((s) => isActiveSlice(s.status)) });
});

test('golden: shortDuration', () => {
  checkGolden('features-model/shortDuration', {
    table: [null, undefined, -1, 0, 999, 1000, 65_000, NaN].map((ms) => [String(ms), shortDuration(ms)]),
    slices: bySlice((s) => shortDuration(s.durationMs)),
  });
});

test('golden: waitingText over every slice', () => {
  const base = { status: 'pending' as SliceStatus, waitsFor: [] as string[], approvalOf: [] as string[], hold: null as string | null, retryQueued: false };
  checkGolden('features-model/waitingText', {
    slices: bySlice((s, d) => [waitingText(s, (id) => d.slices.find((x) => x.id === id)?.status), waitingText(s)]),
    table: [
      base,
      { ...base, retryQueued: true },
      { ...base, hold: 'lease held by 03' },
      { ...base, hold: 'disk', retryQueued: true },
      { ...base, waitsFor: ['01', '02'] },
      { ...base, waitsFor: ['01'], approvalOf: ['01'] },
      { ...base, status: 'running' as SliceStatus, waitsFor: ['01'] },
    ].map((s) => waitingText(s, (id) => (id === '01' ? 'failed' : id === '02' ? 'interrupted' : undefined))),
  });
});

test('golden: failureWord, sliceDuration, attemptDuration over every slice', () => {
  checkGolden('features-model/failureWord', bySlice(failureWord));
  checkGolden(
    'features-model/sliceDuration',
    bySlice((s, d) => [sliceDuration(s, d.now), sliceDuration(s, FIXED_NOW + 3_600_000)])
  );
  checkGolden('features-model/attemptDuration', {
    slices: bySlice((s, d) => s.attempts.map((a) => [attemptDuration(a, d.now), attemptDuration(a, 0)])),
    table: [
      { startedAt: 1000, endedAt: 5000, runMs: null },
      { startedAt: 1000, endedAt: null, runMs: null },
      { startedAt: 1000, endedAt: 500, runMs: null },
      { startedAt: 1000, endedAt: 5000, runMs: 0 },
    ].map((a) => attemptDuration(a, 3000)),
  });
});

test('golden: pausedSummary, nowParts, featureState, progressText over every feature row', () => {
  const pausedTexts = [
    '',
    'by you',
    '1 infrastructure failure in a row',
    '2 infrastructure failures in a row (last: 04: launch failed: boom)',
    '3 infrastructure failures in a row (last: 04 crashed)',
    'x 2 infrastructure failures in a row',
  ];
  checkGolden('features-model/pausedSummary', { table: pausedTexts.map((p) => [p, pausedSummary(p)]), rows: byRow((r) => (r.paused == null ? null : pausedSummary(r.paused))) });
  checkGolden('features-model/nowParts', byRow(nowParts));
  checkGolden('features-model/featureState', byRow(featureState));
  checkGolden('features-model/progressText', byRow(progressText));
  const synthetic = (over: Partial<FeatureRow>): FeatureRow => ({
    ...list.features[0],
    issue: null,
    paused: null,
    approve: [],
    failed: [],
    note: null,
    running: [],
    merged: 0,
    skipped: 0,
    total: 3,
    active: true,
    daemonPid: 1,
    started: true,
    needsYou: false,
    ...over,
  });
  const cases: Record<string, FeatureRow> = {
    idle: synthetic({}),
    issue: synthetic({ issue: 'bad plan' }),
    'not running': synthetic({ daemonPid: null }),
    'not started': synthetic({ started: false, active: false, daemonPid: null }),
    stopped: synthetic({ active: false, daemonPid: null }),
    done: synthetic({ merged: 3 }),
    'done with skips': synthetic({ merged: 2, skipped: 1 }),
    'zero total': synthetic({ total: 0, active: false, daemonPid: null }),
    everything: synthetic({ issue: 'x', paused: '2 infrastructure failures in a row (last: 04: boom', approve: ['03'], failed: ['04'], note: 'disk low', running: ['05', '06'], needsYou: true }),
  };
  checkGolden('features-model/rows-table', Object.fromEntries(Object.entries(cases).map(([n, r]) => [n, { nowParts: nowParts(r), featureState: featureState(r), progressText: progressText(r) }])));
});

test('golden: sortFeatures', () => {
  const refs = refsOf({ list: list.features });
  const extra = [
    { name: 'b', needsYou: false, running: [], finishedAt: 5, updatedAt: 5 },
    { name: 'a', needsYou: false, running: [], finishedAt: 5, updatedAt: 5 },
    { name: 'c', needsYou: false, running: ['01'], finishedAt: null, updatedAt: null },
    { name: 'd', needsYou: true, running: [], finishedAt: 1, updatedAt: 1 },
    { name: 'e', needsYou: false, running: [], finishedAt: null, updatedAt: 9 },
  ];
  checkGolden('features-model/sortFeatures', { list: sortFeatures(list.features), table: sortFeatures(extra).map((r) => r.name) }, refs);
});

test('golden: attemptKindLabel and attemptResult over every attempt', () => {
  const kinds = ['first', 'infra-retry', 'rerun', 'manual-retry', 'some-new-kind', ''];
  checkGolden('features-model/attemptKindLabel', {
    table: kinds.flatMap((k) => [attemptKindLabel(k), attemptKindLabel(k, true)]),
    attempts: bySlice((s) => s.attempts.map((a) => attemptKindLabel(a.kind, a.fresh))),
  });
  const results = [
    null,
    '',
    'abandoned',
    'merged',
    'gate_failed',
    'empty',
    'conflict',
    'quality_failed',
    'stopped',
    'infra_failed',
    'launch_failed',
    'integration_failed',
    'skipped',
    'brand_new_result',
  ];
  checkGolden('features-model/attemptResult', {
    table: results.map((r) => [r, attemptResult(r, false), attemptResult(r, true), attemptResult(r, false, { current: true }), attemptResult(r, false, { replacedBy: 3 })]),
    // As the attempts table calls it.
    attempts: bySlice((s) =>
      s.attempts.map((a, i) => {
        const last = s.attempts[s.attempts.length - 1];
        const next = s.attempts[i + 1];
        return attemptResult(a.result, a === last && isActiveSlice(s.status), { current: a === last && (s.status === 'failed' || s.status === 'interrupted'), replacedBy: next?.n });
      })
    ),
  });
});

test('golden: verdictLine, oneLine, cleanOutput, reasonSummary over every feature text', () => {
  checkGolden('features-model/verdictLine', { texts: Object.fromEntries(texts.map(([n, t]) => [n, verdictLine(t)])), table: TEXT_TABLE.map((t) => verdictLine(t)) });
  checkGolden('features-model/oneLine', { texts: Object.fromEntries(texts.map(([n, t]) => [n, oneLine(t)])), table: TEXT_TABLE.map((t) => oneLine(t)) });
  checkGolden('features-model/cleanOutput', {
    texts: Object.fromEntries(texts.map(([n, t]) => [n, cleanOutput(t)])),
    table: TEXT_TABLE.filter((t): t is string => typeof t === 'string').map(cleanOutput),
  });
  const reasons = [
    null,
    '',
    'stopped',
    'conflict: in src/a.ts',
    'infrastructure: launch failed: the launch exited (1) without printing a run id: zeroshot: connect: (connection refused)',
    'agent: timed out: after 45m',
    'gate failed: tests / ---- / 3 passed, 1 failed',
    'gate failed: ',
    ': : :',
    'a: b',
  ];
  checkGolden('features-model/reasonSummary', { texts: Object.fromEntries(texts.map(([n, t]) => [n, reasonSummary(t)])), table: reasons.map((r) => reasonSummary(r)) });
});

test('golden: isNoiseEvent, eventText, eventTone, eventSlices over every event', () => {
  checkGolden('features-model/isNoiseEvent', Object.fromEntries(allEvents.map(([n, e]) => [n, isNoiseEvent(e)])));
  checkGolden('features-model/eventText', Object.fromEntries(allEvents.map(([n, e]) => [n, eventText(e)])));
  const kinds = [...new Set(allEvents.map(([, e]) => e.kind))].sort();
  checkGolden('features-model/eventTone', Object.fromEntries([...kinds, 'novel'].map((k) => [k, eventTone(k)])));
  const table: Pick<FeatureEvent, 'kind' | 'detail' | 'slice'>[] = [
    { kind: 'launched', detail: 'weird' },
    { kind: 'launched', detail: 'brand-new attempt #3' },
    { kind: 'merged', detail: '' },
    { kind: 'edited', detail: 'max parallel 3' },
    { kind: 'edited', slice: '04', detail: 'spec /abs/path/04.md and deps' },
    { kind: 'waiting', detail: undefined },
    { kind: 'waiting', detail: 'nothing can start: all blocked' },
    { kind: 'infra_failure', detail: '#2: launch failed: boom' },
    { kind: 'infra_failure', detail: '#2: agent: crashed: segfault' },
    { kind: 'run_finished', detail: 'succeeded' },
    { kind: 'interrupted', detail: 'runtime_lost: gone' },
    { kind: 'checkpoint_waiting', detail: 'review 0123456789abcdef, then: zsf approve calc 03' },
    { kind: 'done', detail: 'every slice is merged into main or skipped' },
    { kind: 'some_new_kind', detail: '  padded  ' },
  ];
  checkGolden(
    'features-model/events-table',
    table.map((e) => ({ e, isNoiseEvent: isNoiseEvent(e), eventText: eventText(e) }))
  );
  checkGolden('features-model/eventSlices', {
    events: Object.fromEntries(events.map(([n, b]) => [n, eventSlices(b.events)])),
    table: eventSlices([{ slice: '10' }, { slice: '2' }, {}, { slice: '01' }, { slice: '2' }, { slice: 'b' }, { slice: 'A' }, { slice: '1a' }]),
  });
});

test('golden: specBody over every slice spec', () => {
  checkGolden('features-model/specBody', {
    slices: bySlice((s) => (s.spec == null ? null : specBody(s.spec, s.id))),
    table: [
      ['# 04: Title\nDepends on: 01\n\nBody', '04'],
      ['\n\n## 04 Title\n**Depends on:** 01, 02\nBody\n', '04'],
      ['# 040: Other\nBody', '04'],
      ['# a.b: dotted\nBody', 'a.b'],
      ['# axb: not dotted\nBody', 'a.b'],
      ['Intro\n# 04: Late heading\n  depends ON : x\nBody', '04'],
      ['', '04'],
    ].map(([t, id]) => specBody(t, id)),
  });
});

test('golden: sliceEdges, sliceGraphKey, buildSliceElk over every feature', () => {
  const table = [
    { id: 'a', deps: [], status: 'merged' as SliceStatus },
    { id: 'b', deps: ['a', 'missing'], status: 'failed' as SliceStatus },
    { id: 'c', deps: ['b', 'a', 'a'], status: 'pending' as SliceStatus },
    { id: 'd', deps: ['c', 'e'], status: 'skipped' as SliceStatus },
    { id: 'e', deps: [], status: 'waiting_for_approval' as SliceStatus },
  ];
  const all = { ...Object.fromEntries(details.map(([n, d]) => [n, d.slices])), table };
  checkGolden('features-model/sliceEdges', Object.fromEntries(Object.entries(all).map(([n, s]) => [n, sliceEdges(s)])));
  checkGolden('features-model/sliceGraphKey', Object.fromEntries(Object.entries(all).map(([n, s]) => [n, sliceGraphKey(s)])));
  checkGolden('features-model/buildSliceElk', Object.fromEntries(Object.entries(all).map(([n, s]) => [n, { RIGHT: buildSliceElk(s), DOWN: buildSliceElk(s, 'DOWN') }])));
  checkGolden('features-model/SLICE_CARD', SLICE_CARD);
});

test('golden: roundedPath over ELK-routed slice edges, and a table', async () => {
  const elk = new ELK();
  const out: Record<string, unknown> = {};
  for (const [n, d] of details) {
    for (const direction of ['RIGHT', 'DOWN'] as const) {
      const layout = await elk.layout(buildSliceElk(d.slices, direction));
      out[`${n} ${direction}`] = Object.fromEntries(
        (layout.edges ?? []).map((e) => {
          const section = (e as { sections?: { startPoint: { x: number; y: number }; endPoint: { x: number; y: number }; bendPoints?: { x: number; y: number }[] }[] }).sections?.[0];
          const points = section ? [section.startPoint, ...(section.bendPoints ?? []), section.endPoint] : [];
          return [e.id, roundedPath(points)];
        })
      );
    }
  }
  const table = [
    [],
    [{ x: 0, y: 0 }],
    [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
    ],
    [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 20 },
    ],
    [
      { x: 0, y: 0 },
      { x: 4, y: 0 },
      { x: 4, y: 3 },
      { x: 20, y: 3 },
    ],
    [
      { x: 0, y: 0 },
      { x: 0, y: 0 },
      { x: 5, y: 5 },
    ],
  ];
  checkGolden('features-model/roundedPath', { layouts: out, table: table.map((p) => [roundedPath(p), roundedPath(p, 0), roundedPath(p, 100)]) });
});

test('golden: parseDeps, waitVerb, statText', () => {
  checkGolden(
    'features-model/parseDeps',
    ['', ' ', '01', '01,02', ' 01 , 02 03\n04\t05,,01 ', 'a,A'].map((t) => [t, parseDeps(t)])
  );
  checkGolden(
    'features-model/waitVerb',
    [[], ['06'], ['06', '07'], ['a', 'b', 'c']].map((ids) => waitVerb(ids))
  );
  const stats = apiBodies<FeatureChanges>('features', /\/changes(-diff)?\.json$/).map(([n, c]) => [n, statText(c.stat)]);
  checkGolden('features-model/statText', {
    changes: Object.fromEntries(stats),
    slices: bySlice((s) => (s.worktreeChanges ? statText(s.worktreeChanges) : null)),
    table: [
      { files: [], additions: 0, deletions: 0 },
      { files: [1], additions: 16, deletions: 0 },
      { files: [1, 2], additions: 3, deletions: 4 },
    ].map(statText),
  });
});

function diffParts(diff: string): unknown[] {
  let cursor = 0;
  return splitDiff(diff).map((p) => {
    const at = diff.indexOf(p.text, cursor);
    if (at < 0) return { path: p.path, text: p.text };
    cursor = at + p.text.length;
    return { path: p.path, text: [at, cursor] };
  });
}

test('golden: splitDiff over every feature diff', () => {
  checkGolden('features-model/splitDiff', {
    // Each part's text is a slice of the diff: pinned as its [start, end) offsets in it.
    diffs: Object.fromEntries(diffs.map(([n, d]) => [n, diffParts(d)])),
    table: ['', 'no diff header\n', 'commit abc\n\ndiff --git a/x b/x\n+1\ndiff --git a/old name b/new name\nrename\n', 'diff --git a/a b/b\n', 'diff --git broken header\n'].map((t) => splitDiff(t)),
  });
});
