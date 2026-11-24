// What the onboarding pieces say about the setup check (pure, tested in onboarding.test.ts).
import type { Setup, SetupProblem } from '../generated/discovery';

/** The copyable example on the first-run card (stock Zeroshot 16.1: the task goes in `--input`). */
export const EXAMPLE_COMMAND = [`echo '{"task": "Add a README that says what this project does"}' > task.json`, `zeroshot run --template single-worker --title "Add a README" --input task.json`].join(
  '\n'
);

/** The Zeroshot problem, when Zeroshot is missing or too old (the check zerochat's actions need). */
export function zeroshotProblem(setup: Setup | null): SetupProblem | null {
  if (!setup || setup.zeroshot.meetsMinimum) return null;
  return setup.problems.find((p) => p.check === 'zeroshot') ?? { check: 'zeroshot', message: 'Zeroshot is not usable', fix: null };
}

/** The strip's words: what is wrong, and for a missing Zeroshot what that turns off. */
export function stripText(setup: Setup | null): string | null {
  const problem = zeroshotProblem(setup);
  if (!problem || !setup) return null;
  return setup.zeroshot.found ? problem.message : 'Zeroshot missing: stop, resume and steer are off';
}

/** `zeroshot 16.1.0` (the overview's raw `--version` output) -> `16.1.0`. */
function bareVersion(raw: string): string {
  return raw.replace(/^zeroshot\s+/i, '').trim();
}

/** `16.1.0` -> [16, 1, 0]; null when it is not a dotted version. */
function versionParts(version: string): number[] | null {
  const m = /^(\d+)\.(\d+)(?:\.(\d+))?/.exec(version.trim());
  return m ? [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)] : null;
}

/** `a` is below `b` (both dotted versions). */
function versionBelow(a: number[], b: number[]): boolean {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] < b[i];
  return false;
}

/**
 * Which Zeroshot, in two words: `Zeroshot 16.1.0`. A Zeroshot that steers like 16.1 but prints a
 * lower version (a source build prints its crate version, `0.1.0`) is `Zeroshot built from
 * source`, so it does not read as contradicting the README's "needs 16.1".
 */
export function zeroshotLabel(setup: Setup): string {
  const z = setup.zeroshot;
  if (!z.found) return 'Zeroshot not found';
  if (!z.version) return 'Zeroshot (version unknown)';
  const have = versionParts(z.version);
  const need = versionParts(z.minimum);
  if (z.meetsMinimum && z.steer && have && need && versionBelow(have, need)) return 'Zeroshot built from source';
  return `Zeroshot ${z.version}`;
}

/** `/home/me/.local/state/zeroshot` -> `~/.local/state/zeroshot` (macOS `/Users/me` and `/root` too). */
export function homePath(path: string): string {
  return path.replace(/^(?:\/home\/[^/]+|\/Users\/[^/]+|\/root)(?=\/|$)/, '~');
}

/** The header's quiet line: which Zeroshot and which runs folder, `Zeroshot 16.1.0 · ~/.local/state/zeroshot`. */
export function setupLine(setup: Setup | null): { text: string; title: string } | null {
  if (!setup) return null;
  const z = setup.zeroshot;
  const program = z.found ? `${z.program}${z.version ? ` (prints ${z.version})` : ''}` : `not found: ${z.program}`;
  return {
    text: `${zeroshotLabel(setup)} · ${homePath(setup.runs.stateDir)}`,
    title: `Zeroshot: ${program}\nRuns folder: ${setup.runs.stateDir}`,
  };
}

/** The logo's tooltip: both versions, each named, so neither reads as the other. */
export function brandTitle(setup: Setup | null, overviewVersion: string | null | undefined): string | undefined {
  if (setup) return `zerochat ${setup.version} · ${zeroshotLabel(setup)}`;
  return overviewVersion ? `Zeroshot ${bareVersion(overviewVersion)}` : undefined;
}
