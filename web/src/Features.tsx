/**
 * The Features pages: the list (`#/features`) and a feature's page (`#/features/<name>`).
 * A feature is a branch built by zsf ("Zeroshot features"), which runs the feature's slices as
 * Zeroshot runs in dependency order and merges each onto the feature branch. The pages live in
 * `features/`:
 *
 * - `FeaturesList.tsx`: the list;
 * - `FeaturePage.tsx`: a feature's page, with `FeatureHeader.tsx` and `Attention.tsx` (what needs you);
 * - `SliceGraph.tsx` (cards in `SliceCard.tsx`), `FeatureChanges.tsx`, `FeatureTimeline.tsx`: its sections;
 * - `SlicePanel.tsx`: a slice beside the graph, with `SliceForms.tsx`, `SliceSections.tsx`, `Attempts.tsx`;
 * - `page-hooks.ts`: polling, title, toast, focus and Escape.
 *
 * Their words and layout rules are in `features-model.ts`.
 */
export { FeaturesList } from './features/FeaturesList';
export { FeaturePage } from './features/FeaturePage';
