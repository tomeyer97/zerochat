// A slice waiting for review (zsf approval model B: its run is done, it is not merged): the same
// words everywhere it shows, exactly three actions, Request changes never empty, Skip asks first.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  REVIEW_ACTIONS,
  REVIEW_WAIT,
  attemptKindLabel,
  attemptResult,
  attentionItems,
  attentionSlice,
  depMark,
  eventText,
  needsAttention,
  nowParts,
  requestChangesBody,
  reviewActions,
  skipConsequences,
  sliceCardState,
  sliceEdges,
  sliceStateSentence,
  sliceStatusLabel,
  sliceTone,
  waitingText,
} from '../features-model';
import type { FeatureDetail, FeatureEvent, FeatureRow, SliceView } from '../types';

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

// 03 waits for review (what zsf offers it: approve, retry, skip); 04 comes after it.
const waiting = slice('03', {
  title: 'Friendlier greeting',
  status: 'waiting_for_review',
  checkpoint: true,
  worktree: '/w/03',
  worktreeExists: true,
  prUrl: 'https://github.com/o/r/pull/3',
  actions: ['approve', 'retry', 'skip'],
});
const after = slice('04', { deps: ['03'], waitsFor: ['03'], approvalOf: ['03'] });
const slices = [waiting, after];
const statusOf = (id: string) => slices.find((s) => s.id === id)?.status;

const row: Pick<FeatureRow, 'paused' | 'approve' | 'failed' | 'running' | 'note' | 'merged' | 'skipped' | 'total' | 'active' | 'daemonPid' | 'started' | 'issue'> = {
  paused: null,
  approve: ['03'],
  failed: [],
  running: [],
  note: null,
  merged: 0,
  skipped: 0,
  total: 2,
  active: true,
  daemonPid: 1,
  started: true,
  issue: null,
};
const detail = { ...row, pausedAt: null, finishedAt: null, slices } as unknown as FeatureDetail;

/** The banner line as the Attention list renders it: `<id> <title> <waits><after>.` */
function bannerLine(): string {
  const item = attentionItems(detail).find((i) => i.kind === 'approve');
  assert.ok(item && item.kind === 'approve');
  return `${item.slice} ${item.title} ${item.waits}${item.after}.`;
}

test('a slice waiting for review says "waiting for your review" in every place', () => {
  assert.equal(REVIEW_WAIT, 'waiting for your review');
  const places: Record<string, string> = {
    pill: sliceStatusLabel(waiting.status),
    card: sliceCardState(waiting, 0, true, statusOf).text,
    list: nowParts(row)[0].text,
    panel: sliceStateSentence(waiting, { slices, paused: false, daemonUp: true, now: 0 }),
    dependency: depMark(waiting.status).text,
    timeline: eventText({ ts: 0, kind: 'checkpoint_waiting', slice: '03', detail: 'waiting for review, not merged (approve = check and merge)' } as FeatureEvent).what,
  };
  for (const [where, text] of Object.entries(places)) assert.match(text.toLowerCase(), /waiting for your review/, where);
  assert.equal(places.card, REVIEW_WAIT);
  assert.equal(places.list, '03 waiting for your review');
  assert.equal(places.panel, 'Waiting for your review; not merged yet. 04 starts after it is merged.');
  // The banner, as the brief words it.
  assert.equal(bannerLine(), '03 Friendlier greeting is ready for your review; 04 starts after it is merged.');
  // What comes after it waits for the review, not for an approval of something merged.
  assert.equal(waitingText(after, statusOf), 'waits for your review of 03');
  // Amber, needs you, and the graph starts there.
  assert.equal(sliceTone(waiting.status), 'warn');
  assert.ok(needsAttention(waiting.status));
  assert.equal(attentionSlice(slices), '03');
  assert.deepEqual(
    sliceEdges(slices).map((e) => e.state),
    ['approval']
  );
});

test('nothing says "merged" before the merge happened', () => {
  const texts = [
    sliceStatusLabel(waiting.status),
    sliceCardState(waiting, 0, true, statusOf).text,
    nowParts(row)[0].text,
    sliceStateSentence(waiting, { slices, paused: false, daemonUp: true, now: 0 }),
    bannerLine(),
    eventText({ ts: 0, kind: 'checkpoint_waiting', slice: '03', detail: 'waiting for review, not merged (approve = check and merge)' } as FeatureEvent).detail,
  ];
  for (const text of texts) {
    // Only "not merged yet" and "after it is merged" may name the merge.
    const rest = text.replace(/not merged yet|after it is merged/g, '');
    assert.doesNotMatch(rest, /merged/i, text);
  }
  // The banner offers Review (the panel, with the diff), not an approval from the banner.
  const item = attentionItems(detail)[0];
  assert.ok(item.kind === 'approve' && item.review && item.since === null);
});

test('exactly three actions: Approve (checks, then merge), Request changes, Skip', () => {
  assert.deepEqual(
    reviewActions(waiting)?.map((a) => [a.action, a.label, a.opens]),
    [
      ['approve', 'Approve: run checks and merge', 'now'],
      ['retry', 'Request changes…', 'form'],
      ['skip', 'Skip…', 'dialog'],
    ]
  );
  assert.equal(REVIEW_ACTIONS.length, 3);
  // Only what zsf accepts now, and only while it waits for review.
  assert.deepEqual(
    reviewActions({ ...waiting, actions: ['approve', 'skip'] })?.map((a) => a.action),
    ['approve', 'skip']
  );
  assert.equal(reviewActions({ ...waiting, status: 'failed', actions: ['retry', 'skip'] }), null);
  assert.equal(reviewActions({ ...waiting, status: 'waiting_for_approval', actions: ['approve'] }), null);
});

test('Request changes cannot be sent empty; its notes go with retry', () => {
  assert.equal(requestChangesBody(''), null);
  assert.equal(requestChangesBody('  \n\t '), null);
  assert.deepEqual(requestChangesBody('  Name it max2().\n'), { action: 'retry', notes: 'Name it max2().' });
});

test('Skip asks first and says that what comes after it stops waiting for it', () => {
  assert.equal(reviewActions(waiting)?.find((a) => a.action === 'skip')?.opens, 'dialog');
  assert.equal(skipConsequences(waiting, [after]), "03 is never merged; 04 Slice 04 stops waiting for it and goes ahead without it. This can't be undone here. Its worktree stays where it is.");
  assert.equal(
    skipConsequences(waiting, [after, slice('05')]),
    "03 is never merged; 04 Slice 04, 05 Slice 05 stop waiting for it and go ahead without it. This can't be undone here. Its worktree stays where it is."
  );
});

test('the reviewed attempt reads "changes requested", muted (not a failure); the gate re-run has its words', () => {
  assert.deepEqual(attemptResult('changes_requested', false, { current: true }), { label: 'changes requested', tone: 'muted' });
  assert.equal(attemptKindLabel('gate-rerun'), 're-run after gate failed');
});
