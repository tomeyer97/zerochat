// Batches this tab started a moment ago. Right after Start, pb has not written the new batch's
// row yet, so the batch page asks again instead of saying "not found". This is browser state
// (a module-level map, mirrored in session storage so a reload keeps it), not a pure helper.

/** How long after Start a 404 for the new batch still reads as "starting": `pb run --detach`
 * returns the id before its runner has written the batch's row (seconds, longer on a cold `uv`
 * environment). The server answers `starting` itself when it knows the id; this covers a server
 * that does not (restarted, or an older build). */
export const START_PATIENCE_MS = 3 * 60_000;

const STARTED_KEY = 'zerochat.startedBatches';
const startedHere = new Map<string, number>();

/** The ids this tab started, by start time, from session storage. Throws when session storage
 * is unavailable (callers fall back to the in-memory map). */
function readStored(): Record<string, number> {
  return JSON.parse(sessionStorage.getItem(STARTED_KEY) ?? '{}') as Record<string, number>;
}

/** Remembered for this tab (and across a reload, when session storage works). */
export function noteStarted(id: string, at = Date.now()): void {
  startedHere.set(id, at);
  try {
    const stored = readStored();
    stored[id] = at;
    sessionStorage.setItem(STARTED_KEY, JSON.stringify(stored));
  } catch {
    // no session storage: the in-memory note still covers this tab
  }
}

/** When this tab started `id`, if it did within `START_PATIENCE_MS`. */
export function startedRecently(id: string, now = Date.now()): number | null {
  const at = startedHere.get(id) ?? storedStart(id);
  return at != null && now - at < START_PATIENCE_MS ? at : null;
}

function storedStart(id: string): number | undefined {
  try {
    const at = readStored()[id];
    return typeof at === 'number' ? at : undefined;
  } catch {
    return undefined;
  }
}
