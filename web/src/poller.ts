// A poll loop, free of React so its rules can be read and tested in one place.

/** The timer functions the loop uses; tests pass their own. */
export type Timers = {
  setTimeout: (fn: () => void, ms: number) => unknown;
  clearTimeout: (handle: unknown) => void;
};

const browserTimers: Timers = {
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (handle) => globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>),
};

export type Poller = {
  /** Starts (or restarts after `stop`) the loop with a poll right away. */
  start(): void;
  /** Polls now; if a poll is in flight, polls again as soon as it answers. */
  poke(): void;
  /** Ends the loop: no poll is scheduled after this (one in flight still finishes). */
  stop(): void;
};

/**
 * Calls `poll` over and over, with these rules:
 * - Never two at once: the next poll is scheduled only after the last one answered, so slow
 *   answers never pile up and are never aborted.
 * - After each answer it waits `delay()` milliseconds, read at that moment (so a change of pace
 *   applies from the next wait on).
 * - `poke()` during a poll does not wait out that delay: the next poll starts as soon as the
 *   current one answers.
 * `poll` reports its own errors; a rejection only ends that one poll.
 */
export function createPoller({ poll, delay, timers = browserTimers }: { poll: () => Promise<void>; delay: () => number; timers?: Timers }): Poller {
  let timer: unknown;
  let busy = false;
  let again = false;
  let stopped = true;

  async function tick() {
    if (stopped) return;
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    timers.clearTimeout(timer);
    try {
      await poll();
    } catch {
      // The next poll tries again.
    } finally {
      busy = false;
      const wait = again ? 0 : delay();
      again = false;
      if (!stopped) timer = timers.setTimeout(() => void tick(), wait);
    }
  }

  return {
    start() {
      stopped = false;
      void tick();
    },
    poke() {
      void tick();
    },
    stop() {
      stopped = true;
      timers.clearTimeout(timer);
    },
  };
}
