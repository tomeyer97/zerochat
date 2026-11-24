// The slice graph's and slice panel's words: card state, panel sentence, attention, edit checks,
// attempt reasons, artifacts, confirmations, diff counts and the timeline's run links.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  EDGE_LABEL,
  EDGE_STATES,
  acceptSummary,
  artifactKind,
  attemptReason,
  attemptsHint,
  attentionSlice,
  depMark,
  diffStat,
  editDiff,
  eventLinksRun,
  skipConsequences,
  sliceCardLabel,
  sliceCardMeta,
  sliceCardState,
  sliceStateSentence,
} from './features-model';
import type { AttemptView, SliceView } from './types';

const attempt = (n: number, extra: Partial<AttemptView> = {}): AttemptView => ({
  n,
  kind: 'first',
  fresh: false,
  runId: null,
  runTarget: null,
  runStatus: null,
  earlierRuns: [],
  startedAt: 0,
  endedAt: null,
  result: null,
  reason: null,
  backup: null,
  runMs: null,
  runEnded: false,
  runClosingSince: null,
  runFinishedAt: null,
  hasTask: false,
  costUsd: null,
  costEstimated: false,
  ...extra,
});

const slice = (id: string, extra: Partial<SliceView> = {}): SliceView => ({
  id,
  title: `Slice ${id}`,
  status: 'pending',
  deps: [],
  waitsFor: [],
  approvalOf: [],
  checkpoint: false,
  approved: false,
  accepted: false,
  reason: null,
  commit: null,
  branch: null,
  worktree: null,
  worktreeExists: false,
  notes: [],
  autoNotes: [],
  retryQueued: false,
  hold: null,
  worktreeChanges: null,
  prUrl: null,
  leases: [],
  approvedAt: null,
  skippedAt: null,
  mergedAt: null,
  spec: null,
  specPath: `/specs/${id}.md`,
  attempts: [],
  artifacts: [],
  durationMs: null,
  costUsd: null,
  costEstimated: false,
  actions: [],
  ...extra,
});

const noStatus = () => undefined;

test('sliceCardState: an icon name, a few words and a timer while it runs', () => {
  const merged = slice('01', { status: 'merged', commit: 'abcdef123456' });
  assert.deepEqual(sliceCardState(merged, 0, true, noStatus), { icon: 'done', text: 'merged abcdef1' });
  const retried = { ...merged, accepted: true, attempts: [attempt(1, { kind: 'first' }), attempt(2, { kind: 'manual-retry' })] };
  assert.deepEqual(sliceCardState(retried, 0, true, noStatus), { icon: 'done', text: 'merged without gate' }, 'no sha when the hint needs the room');
  assert.equal(sliceCardState(slice('03', { status: 'waiting_for_approval' }), 0, true, noStatus).icon, 'approval');
  const ended = slice('04', { status: 'running', attempts: [attempt(1, { runEnded: true })] });
  assert.equal(sliceCardState(ended, 0, true, noStatus).text, 'run finished; checks before merge next');
  assert.equal(sliceCardState(ended, 0, false, noStatus).text, 'run finished; merges on Start');
  const second = slice('05', { status: 'running', attempts: [attempt(1), attempt(2, { kind: 'rerun', startedAt: 1000 })] });
  assert.deepEqual(sliceCardState(second, 46_000, true, noStatus), { icon: 'running', text: '#2 re-run after conflict', timer: '45s' });
  assert.deepEqual(sliceCardState(slice('06'), 0, true, noStatus), { icon: null, text: 'ready' });
});

test('attemptsHint: why it took more than one attempt', () => {
  assert.equal(attemptsHint(slice('01', { attempts: [attempt(1)] })), '');
  const kinds = ['first', 'rerun', 'manual-retry', 'manual-retry', 'infra-retry'] as const;
  assert.equal(attemptsHint(slice('01', { attempts: kinds.map((kind, i) => attempt(i + 1, { kind })) })), 're-run (conflict), retried ×2, auto-retried');
});

test('sliceCardMeta: the hint, else time and cost; nothing while running or skipped', () => {
  const done = slice('01', { status: 'merged', attempts: [attempt(1, { runMs: 45_000 })], costUsd: 0.5 });
  assert.equal(sliceCardMeta(done, 0).length, 2);
  assert.deepEqual(sliceCardMeta({ ...done, status: 'running' }, 0), []);
  assert.deepEqual(sliceCardMeta({ ...done, status: 'skipped' }, 0), []);
});

test('sliceCardLabel', () => {
  assert.equal(sliceCardLabel(slice('04', { checkpoint: true, deps: ['01', '02'] }), 'ready'), '04 Slice 04: ready; approval slice; after 01, 02');
});

test('sliceStateSentence: where it stands and what comes next', () => {
  const ctx = (slices: SliceView[]) => ({ slices, paused: false, daemonUp: true, now: 0 });
  const failed = slice('02', { status: 'failed', attempts: [attempt(1, { result: 'gate_failed' })] });
  const after = slice('03', { deps: ['02'] });
  assert.equal(sliceStateSentence(failed, ctx([failed, after])), 'Stopped: gate failed; 03 waits for it.');
  const waiting = slice('02', { status: 'waiting_for_approval', commit: 'abcdef123' });
  assert.equal(sliceStateSentence(waiting, ctx([waiting, after])), 'Merged as abcdef1; waits for your approval before 03 can start.');
  const interrupted = slice('02', { status: 'interrupted' });
  assert.equal(sliceStateSentence(interrupted, { ...ctx([]), paused: true }), 'Interrupted by the pause; it continues in its worktree when you unpause.');
  assert.equal(sliceStateSentence(slice('02', { status: 'merged', accepted: true }), ctx([])), 'Merged without the gate.');
  assert.equal(sliceStateSentence(slice('02', { status: 'skipped' }), ctx([after])), 'Skipped; its work is not merged and 03 went ahead without it.');
  const running = slice('02', { status: 'running', attempts: [attempt(1), attempt(2, { kind: 'manual-retry', fresh: true })] });
  assert.equal(sliceStateSentence(running, ctx([])), 'Running attempt 2 (your retry, from scratch).');
  assert.equal(sliceStateSentence(slice('02'), ctx([])), 'Ready: starts next.');
  assert.equal(sliceStateSentence(slice('03', { waitsFor: ['02'] }), ctx([])), 'Waits for 02.');
});

test('attentionSlice: the first stopped slice or checkpoint', () => {
  assert.equal(attentionSlice([slice('01', { status: 'merged' }), slice('02', { status: 'running' })]), undefined);
  assert.equal(attentionSlice([slice('01'), slice('02', { status: 'interrupted' }), slice('03', { status: 'failed' })]), '02');
});

test('EDGE_STATES: one per edge label', () => {
  assert.deepEqual(EDGE_STATES, ['met', 'open', 'approval', 'blocked', 'skipped']);
  assert.equal(EDGE_LABEL.blocked, 'failed');
});

test('editDiff: checks and only what changed', () => {
  const s = slice('02', { title: 'Old', deps: ['01'], spec: 'Do it.\n' });
  const ids = ['01', '02', '03'];
  const same = editDiff(s, { title: ' Old ', deps: '01', spec: 'Do it.' }, ids);
  assert.deepEqual(same, { titleError: null, depsError: null, body: {}, changed: false, mentionsDeps: false });
  const edited = editDiff(s, { title: 'New', deps: '01, 03 03', spec: 'Depends on: 01\nDo it.' }, ids);
  assert.deepEqual(edited.body, { title: 'New', deps: ['01', '03'], spec: 'Depends on: 01\nDo it.' });
  assert.equal(edited.changed, true);
  assert.equal(edited.mentionsDeps, true);
  assert.equal(editDiff(s, { title: '', deps: '02', spec: '' }, ids).depsError, "02 can't come after itself");
  assert.equal(editDiff(s, { title: '', deps: '09', spec: '' }, ids).depsError, 'not a slice of this feature: 09');
  assert.equal(editDiff(s, { title: '  ', deps: '01', spec: '' }, ids).titleError, "the title can't be empty");
});

test('attemptReason: once, and not the one "Why it stopped" shows', () => {
  const attempts = [
    attempt(1, { result: 'infra_failed', reason: 'infrastructure: launch failed: no run id: connection refused' }),
    attempt(2, { result: 'infra_failed', reason: 'infrastructure: launch failed: no run id: connection refused' }),
    attempt(3, { result: 'conflict', reason: 'conflict in a.py' }),
  ];
  const s = slice('02', { status: 'failed', reason: 'conflict in a.py', attempts });
  assert.equal(attemptReason(s, 0), 'Launch failed: connection refused');
  assert.equal(attemptReason(s, 1), null, 'the row above said the same');
  assert.equal(attemptReason(s, 2), null, 'Why it stopped shows it');
  assert.equal(attemptReason({ ...s, status: 'merged', reason: null }, 2), 'Conflict in a.py');
});

test('artifactKind: Markdown, logs (and the gate hook output), text', () => {
  assert.equal(artifactKind('notes/REPORT.md'), 'markdown');
  assert.equal(artifactKind('gate/output'), 'log');
  assert.equal(artifactKind('run.LOG'), 'log');
  assert.equal(artifactKind('out.txt'), 'log');
  assert.equal(artifactKind('data.json'), 'text');
});

test('depMark', () => {
  assert.deepEqual(depMark('merged'), { text: '✓', tone: 'ok' });
  assert.deepEqual(depMark('waiting_for_approval'), { text: '(needs your approval)', tone: 'warn' });
  assert.deepEqual(depMark(undefined), { text: '', tone: 'muted' });
});

test('skipConsequences and acceptSummary: the confirmations in words', () => {
  const running = slice('02', { status: 'running', attempts: [attempt(1, { runEnded: true })], worktree: '/w/02', worktreeExists: true });
  assert.equal(
    skipConsequences(running, [slice('03'), slice('04')]),
    "Its run already finished; that work is discarded. 02 is never merged; 03 Slice 03, 04 Slice 04 go ahead without it. This can't be undone here. Its worktree stays where it is."
  );
  assert.equal(skipConsequences(slice('02'), [slice('03')]), "02 is never merged; 03 Slice 03 goes ahead without it. This can't be undone here.");
  const changes = { files: [{ path: 'a.py', additions: 3, deletions: 1 }], additions: 3, deletions: 1 };
  assert.equal(acceptSummary(slice('02', { worktreeChanges: changes })), 'Merges its worktree as it is now: 1 file, +3 −1. Use this after you finished it by hand; the gate does not run.');
});

test('diffStat: counts lines, not the file headers', () => {
  const diff = 'diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n-old\n+new\n+more\n';
  const stat = diffStat(diff);
  assert.deepEqual([stat.files.length, stat.additions, stat.deletions], [1, 2, 1]);
});

test('eventLinksRun: the timeline links runs of launches, resumes, lost runs, gate failures and conflicts', () => {
  const linked = ['launched', 'resumed', 'interrupted', 'gate_failed', 'conflict'];
  for (const kind of linked) assert.equal(eventLinksRun(kind), true, kind);
  for (const kind of ['merged', 'run_finished', 'something_new']) assert.equal(eventLinksRun(kind), false, kind);
});
