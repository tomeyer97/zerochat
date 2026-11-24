/**
 * A line diff of `oldText` → `newText`: lines both texts keep become context (` `), the rest
 * `-`/`+`. It is a plain longest-common-subsequence diff, which is quadratic, so above `maxCells`
 * comparisons it gives all old lines then all new ones; a Write-sized edit cannot hang the tab.
 */
export function diffLines(oldText: string, newText: string, maxCells = 250_000): string[] {
  const before = oldText.split('\n');
  const after = newText.split('\n');
  if (before.length * after.length > maxCells) return [...before.map((l) => `-${l}`), ...after.map((l) => `+${l}`)];
  const kept = commonTailLengths(before, after);
  const out: string[] = [];
  let b = 0;
  let a = 0;
  while (b < before.length && a < after.length) {
    if (before[b] === after[a]) {
      out.push(` ${before[b]}`);
      b++;
      a++;
    } else if (kept[b + 1][a] >= kept[b][a + 1]) {
      out.push(`-${before[b]}`);
      b++;
    } else {
      out.push(`+${after[a]}`);
      a++;
    }
  }
  while (b < before.length) out.push(`-${before[b++]}`);
  while (a < after.length) out.push(`+${after[a++]}`);
  return out;
}

/** `kept[b][a]`: how many lines `before[b..]` and `after[a..]` have in common, in order (LCS length). */
function commonTailLengths(before: string[], after: string[]): Uint32Array[] {
  const kept = Array.from({ length: before.length + 1 }, () => new Uint32Array(after.length + 1));
  for (let b = before.length - 1; b >= 0; b--) {
    for (let a = after.length - 1; a >= 0; a--) {
      kept[b][a] = before[b] === after[a] ? kept[b + 1][a + 1] + 1 : Math.max(kept[b + 1][a], kept[b][a + 1]);
    }
  }
  return kept;
}
