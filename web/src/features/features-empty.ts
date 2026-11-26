/**
 * The Features page with no feature yet: what a feature and a slice are, the three steps that make
 * one (zsf init, edit plan.toml, zsf start), and how to get zsf when it is not installed.
 */

/** What a feature and a slice are, in two sentences. */
export const FEATURES_INTRO = [
  'A feature is one change, built on its own branch and cut into slices.',
  'Each slice is one Zeroshot run with its own spec; zsf (Zeroshot features) runs them in dependency order and merges each onto the feature branch.',
] as const;

export type FeatureStep = { label: string; command: string; hint?: string };

/** The three steps, each with a command to copy. */
export const FEATURE_STEPS: readonly FeatureStep[] = [
  {
    label: 'Create it',
    command: 'zsf init NAME --repo PATH --branch NAME --base main',
    hint: 'Add --refined SPEC_DIR to import a spec already cut into slices (slices/INDEX.md + slices/NN-*.md).',
  },
  {
    label: 'List its slices in plan.toml',
    command: '$EDITOR ~/.local/state/zsf/NAME/plan.toml',
    hint: 'One [[slice]] each: id, title, spec, deps; checkpoint = true makes it an approval slice.',
  },
  { label: 'Start it', command: 'zsf start NAME' },
];

/** zerochat's public home (the same URL as `repository` in the root Cargo.toml). */
export const REPO_URL = 'https://github.com/tomeyer97/zerochat';

/** zsf's README on main: the plan format, the commands, how a slice goes. */
export const ZSF_DOCS_URL = `${REPO_URL}/blob/main/crates/zsf/README.md`;

/** How to get zsf when the setup check did not find it: the npm package ships it, or build it from source. */
export const ZSF_INSTALL = {
  message: 'zsf is not installed.',
  /** The one line that says how to get it; the two commands follow it. */
  how: 'It comes with the zerochat npm package; from source, `make` builds it next to zerochat, or install it with cargo:',
  npm: 'npm install -g @zerochat/cli',
  source: 'cargo install --path crates/zsf',
} as const;
