import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPoller, type Timers } from './poller';

/** Timers that only run when the test says so; `waits` records every scheduled delay. */
function manualTimers() {
  const pending = new Map<number, () => void>();
  const waits: number[] = [];
  let next = 1;
  const timers: Timers = {
    setTimeout: (fn, ms) => {
      waits.push(ms);
      pending.set(next, fn);
      return next++;
    },
    clearTimeout: (handle) => void pending.delete(handle as number),
  };
  const fire = () => {
    const [[id, fn]] = pending;
    pending.delete(id);
    fn();
  };
  return { timers, waits, fire, pending };
}

/** A poll whose answers the test releases one by one. */
function controlledPoll() {
  const answers: (() => void)[] = [];
  let calls = 0;
  const poll = () => {
    calls++;
    return new Promise<void>((resolve) => answers.push(resolve));
  };
  const answer = async () => {
    answers.shift()!();
    await new Promise((r) => setImmediate(r));
  };
  return { poll, answer, calls: () => calls };
}

test('poller: polls at start, then waits the delay read after each answer', async () => {
  const t = manualTimers();
  const p = controlledPoll();
  let delay = 4000;
  const poller = createPoller({ poll: p.poll, delay: () => delay, timers: t.timers });
  poller.start();
  assert.equal(p.calls(), 1);
  delay = 30_000;
  await p.answer();
  assert.deepEqual(t.waits, [30_000]);
  t.fire();
  assert.equal(p.calls(), 2);
});

test('poller: never overlaps; a poke during a poll re-polls as soon as it answers', async () => {
  const t = manualTimers();
  const p = controlledPoll();
  const poller = createPoller({ poll: p.poll, delay: () => 4000, timers: t.timers });
  poller.start();
  poller.poke();
  poller.poke();
  assert.equal(p.calls(), 1, 'no second poll while one is in flight');
  await p.answer();
  assert.deepEqual(t.waits, [0], 'the poke turns the wait into 0');
  t.fire();
  assert.equal(p.calls(), 2);
  await p.answer();
  assert.deepEqual(t.waits, [0, 4000], 'the poke is used up');
});

test('poller: a poke between polls polls now and replaces the scheduled one', async () => {
  const t = manualTimers();
  const p = controlledPoll();
  const poller = createPoller({ poll: p.poll, delay: () => 4000, timers: t.timers });
  poller.start();
  await p.answer();
  assert.equal(t.pending.size, 1);
  poller.poke();
  assert.equal(p.calls(), 2);
  assert.equal(t.pending.size, 0, 'the scheduled poll is cleared');
});

test('poller: stop ends the loop, even with a poll in flight; a rejected poll keeps it going', async () => {
  const t = manualTimers();
  const p = controlledPoll();
  const poller = createPoller({ poll: p.poll, delay: () => 4000, timers: t.timers });
  poller.start();
  poller.stop();
  await p.answer();
  assert.equal(t.pending.size, 0);
  poller.poke();
  assert.equal(p.calls(), 1, 'a stopped poller does not poll');

  const failing = createPoller({ poll: () => Promise.reject(new Error('down')), delay: () => 4000, timers: t.timers });
  failing.start();
  await new Promise((r) => setImmediate(r));
  assert.equal(t.pending.size, 1);
});
