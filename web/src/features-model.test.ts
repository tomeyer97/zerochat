import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import {
  attemptResult,
  attemptKindLabel,
  attentionItems,
  buildSliceElk,
  cleanOutput,
  eventSlices,
  eventText,
  eventTone,
  featureState,
  findPaddedSlice,
  hookLines,
  isNoiseEvent,
  isRuleLine,
  nowParts,
  oneLine,
  parseDeps,
  pausedSummary,
  progressText,
  reasonSummary,
  roundedPath,
  sentence,
  shortDuration,
  sliceDuration,
  sliceEdges,
  sliceGraphKey,
  sliceTone,
  sortFeatures,
  specBody,
  splitDiff,
  stoppedReason,
  toastText,
  verdictLine,
  waitingText,
} from './features-model';
import { formatRoute, parseRoute } from './route';
import type { AttemptView, SliceStatus } from './types';

const row = {
  paused: null as string | null,
  approve: [] as string[],
  failed: [] as string[],
  running: [] as string[],
  note: null as string | null,
  merged: 2,
  skipped: 0,
  total: 6,
  active: true,
  daemonPid: 7 as number | null,
  started: true,
  issue: null as string | null,
};
const text = (r: typeof row) => nowParts(r).map((p) => `${p.tone}:${p.text}`);

/** zsf's pause reason after repeated launch failures. */
const PAUSED_REASON = '2 infrastructure failures in a row (last: 03: launch failed: the launch exited (exit status: 75) without printing a run id: zeroshot: x (connection refused))';

describe('nowParts: what needs you first (amber, red only for failed), running blue, done and stopped grey', () => {
  test('approvals, failures, then running', () => {
    assert.deepEqual(text({ ...row, approve: ['02'], failed: ['05'], running: ['06'] }), ['warn:02 waiting for your review', 'bad:05 failed', 'running:running 06']);
  });
  test('paused says how many failures', () => {
    assert.deepEqual(text({ ...row, paused: PAUSED_REASON }), ['warn:paused after 2 failures in a row']);
  });
  test('running, done, done with skipped', () => {
    assert.deepEqual(text({ ...row, running: ['06', '07'] }), ['running:running 06, 07']);
    assert.deepEqual(text({ ...row, merged: 6 }), ['muted:done']);
    assert.deepEqual(text({ ...row, merged: 5, skipped: 1 }), ['muted:done, 1 skipped']);
  });
  test('a stall on disk or a lease is attention, even while something else runs', () => {
    assert.deepEqual(text({ ...row, running: ['03'], note: '04 waits for lease e2e-box' }), ['warn:04 waits for lease e2e-box', 'running:running 03']);
  });
  test('stopped, not started, and a lost daemon', () => {
    assert.deepEqual(text({ ...row, active: false, daemonPid: null }), ['muted:stopped']);
    assert.deepEqual(text({ ...row, active: false, daemonPid: null, started: false }), ['muted:not started']);
    assert.deepEqual(text({ ...row, daemonPid: null }), ['warn:not running']);
  });
});

test('pausedSummary: the count and the slice that failed last', () => {
  assert.deepEqual(pausedSummary(PAUSED_REASON), { text: 'Paused after 2 failures in a row', slice: '03' });
  assert.deepEqual(pausedSummary('by hand'), { text: 'Paused: by hand', slice: null });
});

test('progressText', () => {
  assert.equal(progressText({ merged: 3, skipped: 1, total: 4 }), '3 of 4 merged, 1 skipped');
});

test('featureState: the header chip says Done before Running, and Stopped for an inactive feature', () => {
  const chip = (r: Partial<typeof row> & { needsYou?: boolean }) => featureState({ ...row, needsYou: false, ...r }).label;
  assert.equal(chip({ issue: 'bad plan' }), 'Unreadable');
  assert.equal(chip({ paused: PAUSED_REASON }), 'Paused');
  assert.equal(chip({ needsYou: true }), 'Needs you');
  assert.equal(chip({ merged: 6, running: ['06'] }), 'Done');
  assert.equal(chip({ running: ['06'] }), 'Running');
  assert.equal(chip({ started: false }), 'Not started');
  assert.equal(chip({ active: false }), 'Stopped');
  assert.equal(chip({}), 'Idle');
});

test('sortFeatures: features that need you sort first, then running, then the rest by recency', () => {
  const r = (name: string, needsYou: boolean, running: string[], finishedAt: number | null, updatedAt: number) => ({ name, needsYou, running, finishedAt, updatedAt });
  const sorted = sortFeatures([r('a-done', false, [], 5, 5), r('b-run', false, ['01'], null, 1), r('c-needs', true, [], null, 1), r('d-done', false, [], 9, 9)]);
  assert.deepEqual(
    sorted.map((x) => x.name),
    ['c-needs', 'b-run', 'd-done', 'a-done']
  );
});

test('waitingText says what a pending slice waits on: a blocker, an approval, a lease, a queued retry', () => {
  const s = { status: 'pending' as SliceStatus, waitsFor: [] as string[], approvalOf: [] as string[], hold: null as string | null, retryQueued: false };
  assert.equal(waitingText({ ...s, waitsFor: ['03', '02'], approvalOf: ['03'] }), 'waits for your approval of 03');
  assert.equal(waitingText({ ...s, waitsFor: ['02'] }), 'waits for 02');
  const statusOf = (id: string): SliceStatus => (id === '04' ? 'failed' : 'running');
  assert.equal(waitingText({ ...s, waitsFor: ['02', '04'] }, statusOf), 'blocked by 04 (failed)');
  assert.equal(waitingText(s), 'ready');
  const lease = 'waits for lease box (1 of 1 in use)';
  assert.equal(waitingText({ ...s, hold: lease }), lease);
  assert.equal(waitingText({ ...s, retryQueued: true, hold: 'held while paused' }), 'retry queued · held while paused');
  assert.equal(waitingText({ ...s, status: 'running' }), '');
});

test('sliceTone follows DESIGN: red only failed, amber needs you', () => {
  const tones: [SliceStatus, string][] = [
    ['pending', 'muted'],
    ['running', 'running'],
    ['integrating', 'running'],
    ['merged', 'ok'],
    ['waiting_for_approval', 'warn'],
    ['failed', 'bad'],
    ['interrupted', 'warn'],
    ['skipped', 'muted'],
  ];
  assert.deepEqual(
    tones.map(([status]) => sliceTone(status)),
    tones.map(([, tone]) => tone)
  );
});

test('attemptResult: red only for the current failure; resolved failures muted', () => {
  assert.deepEqual(attemptResult('gate_failed', false, { current: true }), { label: 'gate failed', tone: 'bad' });
  assert.equal(attemptResult('gate_failed', false).tone, 'muted', 'a failure a later attempt moved past is history');
  assert.equal(attemptResult('abandoned', false, { replacedBy: 3 }).label, 'replaced by #3');
  assert.equal(attemptResult('abandoned', false).label, 'stopped');
  assert.equal(attemptResult('skipped', false).label, 'skipped');
  assert.deepEqual(attemptResult(null, true), { label: 'running', tone: 'running' });
});

test('attemptKindLabel', () => {
  assert.equal(attemptKindLabel('manual-retry', true), 'your retry, from scratch');
  assert.equal(attemptKindLabel('rerun'), 're-run after conflict');
});

describe('slice graph', () => {
  const slices = [
    { id: '01', deps: [], status: 'merged' as SliceStatus },
    { id: '03', deps: [], status: 'waiting_for_approval' as SliceStatus },
    { id: '05', deps: [], status: 'failed' as SliceStatus },
    { id: '06', deps: [], status: 'skipped' as SliceStatus },
    { id: '04', deps: ['03', '01', '05', '06', 'gone'], status: 'pending' as SliceStatus },
  ];
  test('sliceEdges: edge states', () => {
    assert.deepEqual(
      sliceEdges(slices).map((e) => `${e.id}:${e.state}`),
      ['03->04:approval', '01->04:met', '05->04:blocked', '06->04:skipped']
    );
  });
  test('buildSliceElk: columns by depth, routed edges', () => {
    const elk = buildSliceElk(slices);
    assert.equal(elk.children?.length, 5);
    assert.equal(elk.edges?.length, 4);
    assert.equal(elk.layoutOptions?.['elk.layered.layering.strategy'], 'LONGEST_PATH_SOURCE');
    assert.equal(elk.layoutOptions?.['elk.edgeRouting'], 'ORTHOGONAL');
  });
  test('roundedPath', () => {
    assert.equal(
      roundedPath([
        { x: 0, y: 0 },
        { x: 10, y: 0 },
      ]),
      'M 0 0 L 10 0'
    );
    assert.match(
      roundedPath([
        { x: 0, y: 0 },
        { x: 20, y: 0 },
        { x: 20, y: 20 },
      ]),
      /^M 0 0 L 14 0 Q 20 0 20 6 L 20 20$/
    );
  });
  test('sliceGraphKey: status changes keep the layout; a new dependency does not', () => {
    assert.equal(sliceGraphKey(slices), sliceGraphKey(slices.map((s) => ({ ...s, status: 'failed' as SliceStatus }))));
    assert.notEqual(sliceGraphKey(slices), sliceGraphKey([...slices.slice(0, 4), { ...slices[4], deps: ['03'] }]));
  });
});

test('reasonSummary: short headline first', () => {
  const cases: [string, string][] = [
    [
      'infrastructure: launch failed: the launch exited (exit status: 75) without printing a run id: zeroshot: simulated outage (connection refused)',
      'Launch failed: simulated outage (connection refused)',
    ],
    ['gate failed: Ran 4 tests / OK / gate: integration check timed out', 'Gate failed: gate: integration check timed out'],
  ];
  for (const [reason, headline] of cases) assert.equal(reasonSummary(reason).headline, headline);
});

describe('hook output', () => {
  test('isRuleLine: any run of - = _ *, or only long ones', () => {
    assert.ok(isRuleLine('  ------ '));
    assert.ok(isRuleLine('**'));
    assert.ok(!isRuleLine('**', 4));
    assert.ok(!isRuleLine(''));
    assert.ok(!isRuleLine('- item'));
  });
  test('hookLines', () => {
    assert.deepEqual(hookLines('====\n  Ran 4 tests \n\n---\nOK'), ['Ran 4 tests', 'OK']);
    assert.deepEqual(hookLines(null), []);
  });
  test('verdictLine: the last line that is not a rule or blank', () => {
    assert.equal(verdictLine('------\nRan 4 tests\nOK\ngate: timed out\n'), 'gate: timed out');
  });
  test('oneLine: joined with a slash', () => {
    assert.equal(oneLine('----\nRan 4 tests\n\nOK\ngate: needs NOTES.md\n'), 'Ran 4 tests / OK / gate: needs NOTES.md');
  });
  test('cleanOutput keeps indentation and short rules', () => {
    assert.equal(cleanOutput('------\n  Ran 4 tests\n\n***\nOK\n'), '  Ran 4 tests\n***\nOK');
  });
});

describe('eventText: events read as words, no enums, no CLI hints', () => {
  const cases: [string, { kind: string; detail?: string; slice?: string }, { what: string; detail: string }][] = [
    [
      "an older zsf's merged checkpoint: its commit, without the CLI hint",
      { kind: 'checkpoint_waiting', detail: 'review f0242c5cf7, then: zsf approve calc 03' },
      { what: 'waiting for your review', detail: 'f0242c5' },
    ],
    [
      'waiting unmerged for review: no detail (nothing is merged yet)',
      { kind: 'checkpoint_waiting', detail: 'waiting for review, not merged (approve = check and merge)' },
      { what: 'waiting for your review', detail: '' },
    ],
    ['a launch says why it ran', { kind: 'launched', detail: 'manual-retry attempt' }, { what: 'started', detail: 'your retry' }],
    ['a feature-level edit is the parallel limit', { kind: 'edited', detail: 'max parallel 3' }, { what: 'parallel limit', detail: '3' }],
    ['a slice edit hides the spec path', { kind: 'edited', slice: '04', detail: 'spec /tmp/x/04.md' }, { what: 'edited', detail: 'spec' }],
    ['a lost controller in words', { kind: 'interrupted', detail: 'zeroshot recorded the run as failed: runtime_lost' }, { what: 'run lost', detail: 'its controller was lost' }],
    ['a kind with no detail shown', { kind: 'quality_failed', detail: 'change_attempts_exhausted' }, { what: 'review rounds ran out', detail: '' }],
    ['a kind from a newer zsf', { kind: 'brand_new', detail: ' x ' }, { what: 'brand new', detail: 'x' }],
    ['a kind named like an Object method', { kind: 'toString', detail: 'x' }, { what: 'toString', detail: 'x' }],
  ];
  for (const [name, event, expected] of cases) test(name, () => assert.deepEqual(eventText(event), expected));
});

test('eventTone: red for failures, amber for what needs a person', () => {
  assert.equal(eventTone('gate_failed'), 'bad');
  assert.equal(eventTone('checkpoint_waiting'), 'warn');
  assert.equal(eventTone('merged'), 'muted');
  assert.equal(eventTone('constructor'), 'muted');
});

test('isNoiseEvent: events that only repeat their neighbour', () => {
  assert.ok(isNoiseEvent({ kind: 'run_finished', detail: 'succeeded' }));
  assert.ok(isNoiseEvent({ kind: 'daemon_started', detail: 'pid 7' }));
  assert.ok(isNoiseEvent({ kind: 'waiting', detail: 'waiting for you: approve 03' }));
  assert.ok(!isNoiseEvent({ kind: 'waiting', detail: 'disk below 25 GB (3 GB free) before 04' }));
  assert.ok(!isNoiseEvent({ kind: 'run_finished', detail: 'failed: runtime_failed' }));
});

test('eventSlices: the timeline chips, in id order', () => {
  assert.deepEqual(eventSlices([{}, { slice: '05' }, { slice: '04' }, { slice: '05' }, { slice: '10' }]), ['04', '05', '10']);
});

test('sliceDuration: each finished run counts its own time', () => {
  const a = (startedAt: number, endedAt: number | null, runMs: number | null = null) => ({ startedAt, endedAt, runMs }) as never;
  assert.equal(sliceDuration({ attempts: [a(0, 1000), a(2000, null)] }, 2500), 1500);
  // A finished run counts its own time, not the time zsf took to notice (a stopped daemon).
  assert.equal(sliceDuration({ attempts: [a(0, 200_000, 37_000)] }, 300_000), 37_000);
  assert.equal(sliceDuration({ attempts: [] }, 1), null);
});

test('shortDuration', () => {
  assert.equal(shortDuration(356), '<1s');
  assert.equal(shortDuration(45_000), '45s');
});

test('parseDeps', () => {
  assert.deepEqual(parseDeps(' 01, 02 03,,01 '), ['01', '02', '03']);
});

test('specBody: without its own heading and Depends on line', () => {
  assert.equal(specBody('# 04: Add farewell()\n\n**Depends on:** 03\n\nDo it.\n', '04'), 'Do it.\n');
  assert.equal(specBody('Intro\n# 04 later\n', '04'), 'Intro\n# 04 later\n');
});

test('feature routes round-trip', () => {
  for (const route of [{ kind: 'features' as const }, { kind: 'feature' as const, name: 'calc-v2' }, { kind: 'feature' as const, name: 'calc v2', slice: '03' }]) {
    assert.deepEqual(parseRoute(formatRoute(route)), route);
  }
  assert.equal(formatRoute({ kind: 'feature', name: 'calc-v2', slice: '03' }), '#/features/calc-v2/slice/03');
  assert.equal(parseRoute('#/features/x/bogus/1').kind, 'unknown');
});

test('a commit diff splits per file with its path', () => {
  const diff = 'diff --git a/VERSION b/VERSION\nnew file mode 100644\n--- /dev/null\n+++ b/VERSION\n@@ -0,0 +1 @@\n+1.0\ndiff --git a/a.py b/b.py\n--- a/a.py\n+++ b/b.py\n';
  const files = splitDiff(diff);
  assert.deepEqual(
    files.map((f) => f.path),
    ['VERSION', 'a.py → b.py']
  );
  assert.ok(files[0].text.startsWith('diff --git a/VERSION') && files[0].text.includes('+1.0'));
});

describe('the feature page', () => {
  const attempt = (runEnded: boolean) => ({ runEnded }) as AttemptView;
  type SliceFields = { deps: string[]; waitsFor: string[]; status: SliceStatus; commit: string | null; mergedAt: number | null; attempts: AttemptView[] };
  const slice = (id: string, extra: Partial<SliceFields> = {}) => ({
    id,
    title: `Slice ${id}`,
    deps: [],
    waitsFor: [],
    status: 'pending' as SliceStatus,
    commit: null,
    mergedAt: null,
    attempts: [],
    ...extra,
  });
  const detail = {
    finishedAt: null,
    issue: null,
    paused: null,
    pausedAt: null,
    daemonPid: 1 as number | null,
    started: true,
    active: true,
    approve: [] as string[],
    failed: [] as string[],
    note: null as string | null,
    slices: [] as ReturnType<typeof slice>[],
  };

  test('stoppedReason: why nothing runs, amber when work waits', () => {
    assert.equal(stoppedReason(detail), null, 'a daemon runs it');
    const stopped = { ...detail, daemonPid: null };
    assert.deepEqual(stoppedReason({ ...stopped, started: false, active: false }), {
      text: 'Not started yet.',
      tone: 'muted',
    });
    assert.deepEqual(stoppedReason(stopped), {
      text: 'Not running: its daemon is gone, so nothing starts or merges.',
      tone: 'warn',
    });
    assert.deepEqual(stoppedReason({ ...stopped, active: false }), {
      text: 'Stopped: nothing new starts until you start again.',
      tone: 'muted',
    });
    // Runs that ended while no daemon watched: they merge on the next start.
    const ended = [slice('02', { status: 'running', attempts: [attempt(true)] }), slice('03', { status: 'running', attempts: [attempt(true)] })];
    assert.deepEqual(stoppedReason({ ...stopped, active: false, slices: ended.slice(0, 1) }), {
      text: 'Stopped: 02 finished and merges when you start again.',
      tone: 'warn',
    });
    assert.equal(stoppedReason({ ...stopped, active: false, slices: ended })?.text, 'Stopped: 02, 03 finished and merge when you start again.');
    assert.equal(stoppedReason({ ...stopped, finishedAt: 5 } as never), null);
  });

  test('attentionItems: paused, approvals, failures, the note, then why nothing runs', () => {
    const items = attentionItems({
      ...detail,
      daemonPid: null,
      paused: PAUSED_REASON,
      pausedAt: 9,
      approve: ['03'],
      failed: ['05'],
      note: 'disk is low',
      slices: [slice('03', { commit: 'f0242c5cf7', mergedAt: 7 }), slice('04', { deps: ['03'] }), slice('05'), slice('06', { waitsFor: ['05'] }), slice('07', { waitsFor: ['05'] })],
    });
    assert.deepEqual(items, [
      { kind: 'paused', text: 'Paused after 2 failures in a row', slice: '03', reason: PAUSED_REASON, since: 9 },
      {
        kind: 'approve',
        slice: '03',
        title: 'Slice 03',
        review: false,
        waits: 'is merged (f0242c5) and waits for your approval',
        since: 7,
        after: '; 04 starts after it',
      },
      { kind: 'failed', slice: '05', title: 'Slice 05', failure: 'failed', after: '; 06, 07 wait for it' },
      { kind: 'note', text: 'Held back: disk is low.' },
    ]);
    assert.deepEqual(attentionItems({ ...detail, daemonPid: null }), [{ kind: 'stopped', text: 'Not running: its daemon is gone, so nothing starts or merges.', tone: 'warn' }]);
  });

  test('toastText and sentence: a full stop, first letter up unless it starts with the feature', () => {
    assert.equal(sentence(' approved 03 '), 'Approved 03.');
    assert.equal(sentence('03 approved!'), '03 approved!');
    assert.equal(sentence(''), '');
    assert.equal(toastText('calc-v2 started', 'calc-v2'), 'calc-v2 started.');
    assert.equal(toastText('calc-v2 stopped!', 'calc-v2'), 'calc-v2 stopped.');
    assert.equal(toastText('resumed', 'calc-v2'), 'Resumed.');
  });

  test('findPaddedSlice: `4` finds 04', () => {
    const slices = [slice('03'), slice('04'), slice('x1')];
    assert.equal(findPaddedSlice(slices, '4')?.id, '04');
    assert.equal(findPaddedSlice(slices, '9'), undefined);
    assert.equal(findPaddedSlice(slices, 'x'), undefined);
  });
});
