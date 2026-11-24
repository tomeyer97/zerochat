/** A commit's or a worktree's diff, as the Features page lists it. */

/** `git show` output split per file, with the path each one changes. */
export function splitDiff(text: string): { path: string; text: string }[] {
  const parts = text.split(/^(?=diff --git )/m).filter((p) => p.trim());
  return parts.map((p) => {
    const m = /^diff --git a\/(.*) b\/(.*)$/m.exec(p);
    return { path: m ? (m[1] === m[2] ? m[2] : `${m[1]} → ${m[2]}`) : '', text: p };
  });
}

/** `2 files, +16 −0`. */
export function statText(stat: { files: unknown[]; additions: number; deletions: number }): string {
  return `${stat.files.length} file${stat.files.length === 1 ? '' : 's'}, +${stat.additions} −${stat.deletions}`;
}

/**
 * A diff's size, counted from its lines (`+`/`-`, not the `+++`/`---` file headers). The commit
 * endpoint sends only the diff; a worktree's changes come with these counts from the server.
 */
export function diffStat(text: string): { files: { path: string; text: string }[]; additions: number; deletions: number } {
  const files = splitDiff(text);
  let additions = 0;
  let deletions = 0;
  for (const f of files) {
    for (const line of f.text.split('\n')) {
      if (line.startsWith('+') && !line.startsWith('+++')) additions++;
      else if (line.startsWith('-') && !line.startsWith('---')) deletions++;
    }
  }
  return { files, additions, deletions };
}
