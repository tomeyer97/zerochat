// Slice 07's words. While zsf runs the merge gate the card, the pill, the panel and the attempt row
// say "checks before merge (build + tests)" with the time since it started (never "merging soon");
// a plan's checkpoint slice is an "approval slice"; the empty Features page explains itself.
import { FIXED_NOW, apiBodies, checkGolden } from '../testdata/golden';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { APPROVAL_SLICE, GATE_FULL, attemptStepLabel, mergeStep, sliceCardLabel, sliceCardState, sliceStateSentence, sliceStateText, sliceStatusLabel } from '../features-model';
import type { AttemptView, FeatureDetail, SliceStatus, SliceView } from '../types';
import type { Setup } from '../generated/discovery';
import { timeOfDay } from '../format';
import { SliceCard } from './SliceCard';
import { Attempts } from './Attempts';
import { FeaturesEmpty } from './FeaturesEmpty';
import { FEATURE_STEPS, ZSF_DOCS_URL, ZSF_INSTALL } from './features-empty';

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
const text = (html: string) =>
  html
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x27;/g, "'")
    .replace(/\s+/g, ' ');
const ctx = (s: SliceView, daemonUp = true, now = 0) => ({ slices: [s], paused: false, daemonUp, now });

// The run ended at 10:00:00 UTC; the gate has run for 2m 05s.
const ENDED = Date.UTC(2026, 10, 22, 10, 0, 0);
const NOW = ENDED + 125_000;
const gating = slice('02', { status: 'integrating', attempts: [attempt(1, { startedAt: ENDED - 600_000, endedAt: ENDED, runEnded: true, runMs: 600_000 })] });

test('F38: the merge gate is "checks before merge (build + tests) · <elapsed>" in the pill and the card', () => {
  assert.deepEqual(mergeStep(gating), { step: 'gate', since: ENDED });
  assert.equal(sliceStateText(gating, NOW, true), 'checks before merge (build + tests) · 2m 05s');
  const card = sliceCardState(gating, NOW, true, noStatus);
  assert.deepEqual(card, { icon: 'running', text: GATE_FULL, timer: '· 2m 05s', wraps: true });
  assert.equal(`${card.text} ${card.timer}`, sliceStateText(gating, NOW, true), 'the card says what the pill says');
  const html = text(renderToStaticMarkup(createElement(SliceCard, { slice: gating, selected: false, now: NOW, daemonUp: true, statusOf: noStatus })));
  assert.match(html, /checks before merge \(build \+ tests\) · 2m 05s/);
  // A re-run (#2) in the gate says the same, not its attempt kind.
  const rerun = { ...gating, attempts: [attempt(1, { result: 'conflict', endedAt: ENDED - 1000 }), attempt(2, { kind: 'rerun', endedAt: ENDED, runEnded: true })] } as SliceView;
  assert.equal(sliceCardState(rerun, NOW, true, noStatus).text, GATE_FULL);
});

test('F38: the panel sentence and the attempt row agree with the pill', () => {
  assert.equal(sliceStateSentence(gating, ctx(gating, true, NOW)), `Checks before merge (build + tests) since ${timeOfDay(ENDED, NOW)}; it merges when they pass.`);
  assert.equal(attemptStepLabel(gating), 'checks before merge');
  const rows = text(renderToStaticMarkup(createElement(Attempts, { feature: 'calc', slice: gating, now: NOW })));
  assert.match(rows, /checks before merge/);
  assert.doesNotMatch(rows, /\brunning\b/, 'the attempt row no longer says running');
});

test('F38: an approved approval slice counts the gate from the approval, not from its run', () => {
  const approvedAt = ENDED + 3_600_000;
  const approved = { ...gating, checkpoint: true, approved: true, approvedAt };
  assert.deepEqual(mergeStep(approved), { step: 'gate', since: approvedAt });
  assert.equal(sliceStateText(approved, approvedAt + 45_000, true), 'checks before merge (build + tests) · 45s');
});

test('F38: without a start time the words stay, without a timer', () => {
  const unknown = { ...gating, attempts: [attempt(1)] };
  assert.equal(sliceStateText(unknown, NOW, true), GATE_FULL);
  assert.equal(sliceCardState(unknown, NOW, true, noStatus).timer, undefined);
  assert.equal(sliceStateSentence(unknown, ctx(unknown)), 'Checks before merge (build + tests); it merges when they pass.');
});

test('F38: Merge worktree (accept) merges without the gate and does not claim checks', () => {
  const accepting = { ...gating, attempts: [attempt(1, { result: 'gate_failed', endedAt: ENDED })] };
  assert.deepEqual(mergeStep(accepting), { step: 'accept' });
  assert.equal(sliceStateText(accepting, NOW, true), 'merging without gate');
  assert.equal(sliceStateSentence(accepting, ctx(accepting)), 'Merging its worktree without the gate.');
  assert.equal(attemptStepLabel(accepting), null, 'the row keeps its result');
});

test('F38: a run that ended before zsf took it up: "run finished" in the pill, card and row; never "merging soon"', () => {
  const ended = slice('04', { status: 'running', attempts: [attempt(1, { runEnded: true })] });
  assert.equal(sliceStateText(ended, 0, true), 'run finished; checks before merge next');
  assert.equal(sliceCardState(ended, 0, true, noStatus).text, 'run finished; checks before merge next');
  assert.equal(attemptStepLabel(ended), 'run finished');
  assert.equal(sliceStateSentence(ended, ctx(ended)), 'Its run finished; zsf takes it up on its next look (every 30s), then runs the checks before merge (build + tests).');
  const review = { ...ended, checkpoint: true };
  assert.equal(sliceStateText(review, 0, true), 'run finished; review next');
  assert.equal(sliceStateSentence(review, ctx(review)), 'Its run finished; zsf takes it up on its next look (every 30s), then it waits for your review.');
  assert.equal(sliceStateText(ended, 0, false), 'run finished; merges on Start');
});

test('R9: the hand-off from a finished run to zsf shows its time, and says why past a few polls', () => {
  // Zeroshot finished the run 20 s ago: within zsf's polls (30 s each), it takes it up next.
  const ended = (ago: number) => slice('06', { status: 'running', attempts: [attempt(1, { runEnded: true, runFinishedAt: NOW - ago })] });
  const soon = ended(20_000);
  assert.deepEqual(mergeStep(soon), { step: 'ended', review: false, closingSince: null, finishedSince: NOW - 20_000 });
  assert.equal(sliceStateText(soon, NOW, true), 'run finished; checks before merge next · 20s');
  assert.deepEqual(sliceCardState(soon, NOW, true, noStatus), { icon: 'done', text: 'run finished; checks before merge next', timer: '· 20s' });
  assert.equal(
    sliceStateSentence(soon, { ...ctx(soon, true, NOW), feature: 'calc' }),
    'Its run finished 20s ago; zsf takes it up on its next look (every 30s), then runs the checks before merge (build + tests).'
  );
  // Past three polls, zsf is stopped, frozen or stuck: say so, and how to restart it.
  const late = ended(95_000);
  assert.equal(sliceStateText(late, NOW, true), 'waiting for zsf · 1m 35s');
  assert.equal(sliceCardState(late, NOW, true, noStatus).timer, '· 1m 35s');
  assert.equal(
    sliceStateSentence(late, { ...ctx(late, true, NOW), feature: 'calc' }),
    'Its run finished 1m 35s ago, and zsf, which looks every 30s, has not taken it up: it may be stuck. Restart it: `zsf stop calc`, then `zsf start calc`.'
  );
  // The feature's own poll counts: 95 s is under three 60 s polls.
  assert.equal(sliceStateText(late, NOW, true, 60), 'run finished; checks before merge next · 1m 35s');
  assert.match(sliceStateSentence(late, { ...ctx(late, true, NOW), feature: 'calc', pollSecs: 60 }), /next look \(every 60s\)/);
  // No daemon: the time, and how to start it.
  assert.equal(sliceStateText(late, NOW, false), 'run finished; merges on Start · 1m 35s');
  assert.equal(
    sliceStateSentence(late, { ...ctx(late, false, NOW), feature: 'calc' }),
    'Its run finished 1m 35s ago, but zsf is not running: it merges when you start the feature (Start, or `zsf start calc`).'
  );
});

test('F38: while Zeroshot finishes the run (its postmortem), the slice says so with the time, not "checks start shortly"', () => {
  const closing = slice('04', { status: 'running', attempts: [attempt(1, { runEnded: true, runClosingSince: NOW - 12_000 })] });
  assert.deepEqual(mergeStep(closing), { step: 'ended', review: false, closingSince: NOW - 12_000, finishedSince: null });
  assert.equal(sliceStateText(closing, NOW, true), 'Zeroshot finishing the run · 12s');
  const card = sliceCardState(closing, NOW, true, noStatus);
  assert.equal(card.text, 'Zeroshot finishing the run');
  assert.equal(card.timer, '· 12s');
  assert.equal(attemptStepLabel(closing), 'run finished');
  assert.equal(sliceStateSentence(closing, ctx(closing, true, NOW)), 'Its agents finished; Zeroshot is finishing the run (its postmortem), then the checks before merge (build + tests).');
  const review = { ...closing, checkpoint: true };
  assert.equal(sliceStateSentence(review, ctx(review, true, NOW)), 'Its agents finished; Zeroshot is finishing the run (its postmortem), then your review.');
  assert.equal(sliceStateText(closing, NOW, false), 'run finished; merges on Start');
});

test('F38: no "merging soon" or bare "merging" for any status', () => {
  const statuses: SliceStatus[] = ['pending', 'launching', 'running', 'integrating', 'merged', 'waiting_for_review', 'waiting_for_approval', 'failed', 'interrupted', 'skipped'];
  const variants = statuses.flatMap((status) => [
    slice('01', { status }),
    slice('01', { status, attempts: [attempt(1, { runEnded: true, endedAt: ENDED })] }),
    slice('01', { status, checkpoint: true, attempts: [attempt(1, { runEnded: true, endedAt: ENDED })] }),
  ]);
  for (const s of variants) {
    for (const daemonUp of [true, false]) {
      const words = [sliceStatusLabel(s.status), sliceStateText(s, NOW, daemonUp), sliceCardState(s, NOW, daemonUp, noStatus).text, sliceStateSentence(s, ctx(s, daemonUp, NOW))];
      for (const w of words) {
        assert.doesNotMatch(w, /merging soon|merges it shortly|Running the gate/i, `${s.status}: ${w}`);
        if (s.status === 'integrating') assert.match(w, /checks before merge|Checks before merge/, w);
      }
    }
  }
});

test('F03: a plan checkpoint slice is an "approval slice" in the card, its tooltip and its label', () => {
  const s = slice('03', { checkpoint: true, deps: ['01'] });
  assert.equal(sliceCardLabel(s, 'ready'), '03 Slice 03: ready; approval slice; after 01');
  const html = renderToStaticMarkup(createElement(SliceCard, { slice: s, selected: false, now: 0, daemonUp: true, statusOf: noStatus }));
  assert.match(html, /title="approval slice: waits for your review before it merges"/);
  assert.match(html, /aria-label="approval slice"/);
  // The class name stays; no visible or spoken word says checkpoint.
  assert.doesNotMatch(html.replace(/class="[^"]*"/g, ''), /checkpoint/i);
  const approved = renderToStaticMarkup(createElement(SliceCard, { slice: { ...s, approved: true }, selected: false, now: 0, daemonUp: true, statusOf: noStatus }));
  assert.match(approved, /title="approval slice, approved"/);
  assert.equal(APPROVAL_SLICE, 'approval slice');
});

const setup = (found: boolean): Setup => ({ zsf: { found, program: found ? '/usr/bin/zsf' : null, version: null, features: 0 } }) as unknown as Setup;

test('F23: the empty Features page says what a feature and a slice are, the three commands and the docs link', () => {
  const html = renderToStaticMarkup(createElement(FeaturesEmpty, { setup: setup(true) }));
  const words = text(html);
  assert.match(words, /A feature is one change/);
  assert.match(words, /Each slice is one Zeroshot run/);
  assert.match(words, /zsf \(Zeroshot features\)/, 'zsf is named in full the first time');
  assert.match(FEATURE_STEPS[0].command, /^zsf init NAME /);
  assert.match(FEATURE_STEPS[1].command, /plan\.toml$/);
  assert.match(FEATURE_STEPS[2].command, /^zsf start NAME$/);
  for (const step of FEATURE_STEPS) assert.ok(words.includes(step.command), step.command);
  assert.equal((html.match(/<li>/g) ?? []).length, 3);
  assert.match(words, /--refined SPEC_DIR to import a spec already cut into slices/);
  assert.ok(html.includes(`href="${ZSF_DOCS_URL}"`));
  assert.doesNotMatch(words, /not installed/);
});

test('F23: when zsf is not found, it says how to get it (npm package or cargo from source)', () => {
  const words = text(renderToStaticMarkup(createElement(FeaturesEmpty, { setup: setup(false) })));
  assert.match(words, /zsf is not installed/);
  assert.ok(words.includes(ZSF_INSTALL.npm));
  assert.ok(words.includes('cargo install --path crates/zsf'));
  // Before the setup check answers, nothing is claimed.
  assert.doesNotMatch(text(renderToStaticMarkup(createElement(FeaturesEmpty, { setup: null }))), /not installed/);
});

test('golden: the pill / card words and the attempt row label over every slice', () => {
  const details = apiBodies<FeatureDetail>('features', /\/detail\.json$/);
  const out: Record<string, unknown> = {};
  for (const [name, d] of details) {
    for (const s of d.slices) {
      const daemonUp = d.daemonPid != null;
      out[`${name.replace(/\/detail$/, '')}/${s.id}`] = { pill: sliceStateText(s, FIXED_NOW, daemonUp), card: sliceCardState(s, FIXED_NOW, daemonUp, noStatus), row: attemptStepLabel(s) };
    }
  }
  out['synthetic/gate'] = { pill: sliceStateText(gating, NOW, true), card: sliceCardState(gating, NOW, true, noStatus), row: attemptStepLabel(gating) };
  checkGolden('features-model/sliceStateText', out);
});
