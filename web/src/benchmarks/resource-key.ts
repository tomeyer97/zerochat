// One format for the keys the Benchmarks pages load and cache under.

/** A cache key from its parts (a missing part counts as empty). The parts are joined with NUL,
 * which no id, label or path contains, so two different part lists never share a key. */
export function resourceKey(...parts: (string | null | undefined)[]): string {
  return parts.map((p) => p ?? '').join('\u0000');
}
