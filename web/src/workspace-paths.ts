/** The directory every absolute path in the tool calls shares (the workspace), if any. */
export function commonDir(paths: string[]): string | null {
  const abs = paths.filter((p) => p.startsWith('/'));
  if (abs.length < 2) return null;
  const split = abs.map((p) => p.split('/'));
  const out: string[] = [];
  for (let i = 0; ; i++) {
    const part = split[0][i];
    // Keep at least the file name of every path.
    if (part === undefined || split.some((s) => s[i] !== part || i >= s.length - 1)) break;
    out.push(part);
  }
  const dir = out.join('/');
  return dir.split('/').length > 2 ? dir : null;
}

/** `/work/x/src/a.ts` → `src/a.ts` inside the workspace `/work/x`. */
export function relativeTo(path: string, workspace: string | null): string {
  if (!workspace || !path.startsWith(workspace)) return path;
  const rest = path.slice(workspace.length).replace(/^\/+/, '');
  return rest || '.';
}
