// Home's row subtitle: the failure cause, else the API's context line for runs whose titles repeat.
import type { RunListing } from '../types';

/** The titles more than one of `runs` share. */
export function repeatedTitles(runs: Pick<RunListing, 'title'>[]): Set<string> {
  const seen = new Set<string>();
  const repeated = new Set<string>();
  for (const r of runs) (seen.has(r.title) ? repeated : seen).add(r.title);
  return repeated;
}

/** The row's subtitle: its `cause`, else (title shared with another run) its context line. */
export function rowSubtitle(run: Pick<RunListing, 'title' | 'context'>, cause: string, repeated: Set<string>): string {
  if (cause) return cause;
  return repeated.has(run.title) ? (run.context?.line ?? '') : '';
}
