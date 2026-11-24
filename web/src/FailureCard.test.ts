// The failure card's next step, rendered from the API snapshots of the server's runs
// (crates/zerochat/testdata/golden/api/runs/*/run.json): the fix leads when a replay cannot
// help, a provider fault is classed Provider, a usage limit says when it resets, and
// "no checkpoints" shows only when the run has none.
import { FIXED_NOW, apiFile } from './testdata/golden';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { FailureCard } from './FailureCard';
import { actionsNote, failureClass, fixLeads, resetNote, unchangedFails, unchangedWarning } from './failure-class';
import type { RunView } from './types';

const run = (name: string) => apiFile<RunView>(`runs/${name}/run.json`).body;

function card(view: RunView, now = FIXED_NOW): string {
  const props = { failure: view.failure!, view, now, overview: undefined, onReplay: () => {}, onResume: () => {} };
  return renderToStaticMarkup(createElement(FailureCard, props));
}

/** The card's text, tags dropped. */
const text = (html: string) => html.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ');
/** The primary button's label, if any. */
const primary = (html: string) => {
  const m = html.match(/<button[^>]*class="button primary"[^>]*>(.*?)<\/button>/);
  return m ? text(m[1]).trim() : null;
};

test('an unknown model leads with the fix, not a replay', () => {
  const html = card(run('fork-only/fork-unknown-model'));
  assert.equal(primary(html), null, 'no primary button');
  assert.match(text(html), /Fix: Set a model Anthropic knows for worker in your runtime file, then start a new run\./);
  assert.ok(!/Replay from worker|Start again from worker/.test(html), html);
  assert.ok(fixLeads(run('fork-only/fork-unknown-model').failure!));
});

test('a node timeout leads with the fix, not a replay', () => {
  for (const name of ['fork-only/fork-worker-timeout', 'fork-only/fork-late-failure', 'diag-code-timeout']) {
    const html = card(run(name));
    assert.equal(primary(html), null, name);
    assert.match(text(html), /Fix: Raise timeoutMs on \w+ in your graph \(now \d+s\)/, name);
  }
});

test('start again unchanged sits behind the fix, and its dialog warns it fails the same way', () => {
  for (const name of ['fork-only/fork-unknown-model', 'fork-only/fork-worker-timeout', 'zeroshot-16/zeroshot16-unknown-model']) {
    const view = run(name);
    assert.ok(unchangedFails(view.failure!), name);
    assert.match(unchangedWarning(view.failure!) ?? '', /^Unchanged, this will fail the same way\. Fix first: /, name);
    const canEdit = (view.capabilities ?? []).includes('replay');
    const html = card(view);
    if (!canEdit) assert.ok(!/class="button"[^>]*>[^<]*<svg[^>]*>.*?Start again/.test(html), `${name}: start again is not a full button`);
    assert.match(html, /class="text-button demoted"[^>]*>.*?Start again/, name);
  }
  assert.equal(unchangedWarning({ primary: 'resume', fix: null, class: 'provider', cause: 'usage limit' }), null);
});

test('a reviewer that rejects every round leads with the fix', () => {
  const html = card(run('fork-only/fork-reviewer-never-satisfied'));
  assert.equal(primary(html), null);
  assert.match(text(html), /Fix: Raise maxIterations on/);
});

test('"no checkpoints" only when the checkpoint list is empty', () => {
  assert.match(text(card(run('no-checkpoints'))), /no checkpoints/);
  // The fixture is a feature slice's run (its card links the slice); the same run on its own.
  const stalled = card({ ...run('stalled'), context: null });
  assert.ok(!/no checkpoints/.test(stalled), 'a stalled run has checkpoints: Resume only');
  assert.equal(primary(stalled), 'Resume');
  assert.ok(!/no checkpoints/.test(card(run('fork-only/fork-worker-timeout'))));
});

test('while Zeroshot closes the run, the card says actions come in a moment', () => {
  const view = run('closing-postmortem');
  assert.equal(actionsNote(view), 'Zeroshot is finishing the run: actions in a moment');
  const html = card(view);
  assert.match(text(html), /Zeroshot is finishing the run: actions in a moment/);
  assert.ok(!/no checkpoints/.test(html));
});

test('a usage limit says when Resume will work', () => {
  const view = run('diag-usage-limit');
  const at = view.failure!.resetsAt!;
  const before = card(view, at - 12 * 60_000);
  assert.equal(primary(before), 'Resume at 9:30 AM');
  assert.match(text(before), /\(limit resets in 12 min\)/);
  const after = card(view, at + 60_000);
  assert.equal(primary(after), 'Resume');
  assert.match(text(after), /limit reset at 9:30 AM/);
  assert.equal(resetNote({ resetsAt: null }, at), null);
});

test('provider faults are classed Provider, not Agent', () => {
  for (const name of ['diag-usage-limit', 'diag-rate-limit', 'diag-provider-down', 'diag-provider-timeout', 'diag-malformed-stream', 'diag-stream-cut']) {
    const view = run(name);
    assert.equal(view.failure!.class, 'provider', name);
    assert.equal(view.failure!.primary, 'resume', name);
    const html = card(view);
    assert.match(html, /class="failure-class tone-text-warn"[^>]*>Provider</, name);
    assert.ok(!/errored/.test(text(html)), `${name}: no stray word under Error output`);
  }
  assert.equal(failureClass('provider').label, 'Provider');
});

test('with stock Zeroshot the card offers Start again from…, never Replay', () => {
  for (const name of ['fork-only/fork-unknown-model', 'fork-only/fork-worker-timeout', 'diag-code-timeout', 'diag-rate-limit']) {
    const view = run(name);
    if (!view.failure?.actions.includes('replay')) continue;
    const html = card(view);
    assert.match(text(html), /Start again from/, name);
    assert.doesNotMatch(text(html), /Replay/, name);
  }
});
