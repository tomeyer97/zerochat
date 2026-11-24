/**
 * Hook output (gate, after-merge and other zsf hooks) as the Features page shows it. Hooks print
 * free text, often framed by rule lines (`-----`, `=====`); those frames carry nothing.
 */

/**
 * A line that is only a rule: `-`, `=`, `_` or `*` repeated, at least `minLength` times.
 * The one-line helpers drop any run; `cleanOutput` shows the output whole, so it drops only long
 * rules and keeps short runs such as `--` or `***`, which can be part of the text.
 */
export function isRuleLine(line: string, minLength = 1): boolean {
  const t = line.trim();
  return t.length >= minLength && /^[-=_*]+$/.test(t);
}

/** The lines worth reading: trimmed, without blank and rule lines. */
export function hookLines(text: string | null | undefined): string[] {
  return (text ?? '')
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !isRuleLine(l));
}

/** The useful line of a hook's output: the last line that is not a rule or blank. */
export function verdictLine(text: string | null | undefined): string {
  const lines = hookLines(text);
  return lines[lines.length - 1] ?? '';
}

/** One line of a multi-line hook output: blank lines and rules dropped, joined with ` / `. */
export function oneLine(text: string | null | undefined): string {
  return hookLines(text).join(' / ');
}

/** Hook output without blank lines and rule lines of 4 or more; lines keep their indentation. */
export function cleanOutput(text: string): string {
  return text
    .split('\n')
    .filter((l) => l.trim() && !isRuleLine(l, 4))
    .join('\n');
}
