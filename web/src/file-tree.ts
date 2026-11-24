/** The Files tab's folder tree, and which changed files it hides as generated artefacts. */

/** One entry of the nested folder listing the file browser shows. */
export type TreeEntry = { name: string; path: string; dir: boolean; children?: TreeEntry[] };

/** Builds a nested folder listing from flat paths: folders first, then by name. */
export function buildTree(paths: string[]): TreeEntry[] {
  const root: TreeEntry = { name: '', path: '', dir: true, children: [] };
  for (const path of paths) {
    const parts = path.split('/');
    let node = root;
    parts.forEach((part, i) => {
      const leaf = i === parts.length - 1;
      node.children ??= [];
      let child = node.children.find((c) => c.name === part && c.dir === !leaf);
      if (!child) {
        child = { name: part, path: parts.slice(0, i + 1).join('/'), dir: !leaf, children: leaf ? undefined : [] };
        node.children.push(child);
      }
      node = child;
    });
  }
  const sort = (entries: TreeEntry[]) => {
    entries.sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name));
    for (const e of entries) if (e.children) sort(e.children);
  };
  sort(root.children!);
  return root.children!;
}

// Generated files (bytecode, caches, installed dependencies) that a change list hides by default.
// The web decides this alone; the server sends every changed path.
/** Folders whose whole content is generated. */
const ARTEFACT_DIRS = ['__pycache__', 'node_modules', '.pytest_cache', '.mypy_cache', '.ruff_cache', '.tox', '.venv'];
/** Folder name endings that mark generated folders (`mypkg.egg-info/`). */
const ARTEFACT_DIR_SUFFIXES = ['.egg-info'];
/** Generated file names. */
const ARTEFACT_FILES = ['.DS_Store'];
/** Generated file extensions. */
const ARTEFACT_EXTS = ['.pyc', '.pyo', '.tsbuildinfo'];

export function isArtefact(path: string): boolean {
  const folders = path.split('/');
  const name = folders.pop()!;
  return (
    folders.some((dir) => ARTEFACT_DIRS.includes(dir) || ARTEFACT_DIR_SUFFIXES.some((suffix) => dir.endsWith(suffix))) ||
    ARTEFACT_FILES.includes(name) ||
    ARTEFACT_EXTS.some((ext) => name.endsWith(ext))
  );
}
