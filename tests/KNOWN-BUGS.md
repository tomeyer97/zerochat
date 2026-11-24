# Known bugs pinned by the tests

The suites pin today's behaviour. These are behaviours found while writing them that look wrong;
they are pinned as they are (not fixed while writing the tests). Fixing one means updating its goldens
on purpose.

| Where | What | Pinned in |
|---|---|---|
| benchmarks (batch totals) | `totalCostUsd` and similar sums are added in HashMap order, so the last bits of the float differ from run to run (`31.803607049999997` vs `31.80360705`). The goldens round floats to 9 decimals to stay stable. | `api/benchmarks/batches`, `benchmarks/direct/*` |
| server `workspace_of` | Finding the repository of a removed zsf slice worktree uses zsf's own state root (`ZSF_STATE_DIR`/XDG), not `zsf_state_dir` from zerochat's config.toml, so with a configured zsf root a merged slice's run shows "workspace unavailable" instead of its last snapshot. | not pinned (found by reading `server.rs`; needs a setup where only config.toml names the zsf root) |
| features (detail of an unreadable feature) | `GET /api/features/{name}` for a feature whose feature.toml is invalid answers 500 `internal` with the full filesystem path and the TOML parser's multi-line message, instead of a 4xx/the list's issue text. | `api/features/broken/detail` |
| features (start without a zsf program) | `POST /api/features/{name}/actions {"action":"start"}` with no zsf program answers "Can't start calc: in another state now." The real reason (no `zsf` program to run the daemon) is only in the code path behind it. | `api/post/feature-start-without-zsf` |
| web `transcript-model.relativeTo` | Matches the workspace as a plain string prefix: `/w/xy/a.ts` inside workspace `/w/x` becomes `y/a.ts`. | `web/src/testdata/golden/transcript-model/*` |
| web `markdown.parseInline` | A link's href stops at the first `)` (`https://example.com/a_(b)` → `…a_(b` plus a stray `)`); `[outer [inner](a)](b)` gives the link text `outer [inner`. | `web/src/testdata/golden/markdown/*` |
| web `format.usd` | Any negative value prints `<$0.01` (e.g. -1500). | `web/src/testdata/golden/format/*` |
| web `benchmarks-model.totalsVerdict` | A `beyond_spread` task with Δ=0, or an unknown verdict, is counted as compared but in no bucket: text `''`, verdict `within_noise`. | `web/src/testdata/golden/benchmarks-model/*` |
| web `graph-model.shortLabel`, `format.truncate` | `shortLabel(label, 0)` returns nearly the whole label plus `…`; `truncate(text, 0)` returns `…` (longer than the maximum); both cut by UTF-16 units and can leave a lone surrogate before the `…`. | `web/src/testdata/golden/{graph-model,format}/*` |
| web `format.test.ts` (existing test) | Depends on the host locale/time zone: fails under `TZ=America/Los_Angeles` with a German locale. The golden tests fix TZ=UTC, `Date.now()` and `en-US` themselves (`web/src/testdata/golden.ts`). | — |
