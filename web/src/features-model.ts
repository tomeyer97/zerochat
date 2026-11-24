/**
 * The Features page's model: pure functions from the API's feature views to words, colours and
 * layout. Features are zsf's ("Zeroshot features": the scheduler that runs a feature's slices as
 * Zeroshot runs and merges them onto one branch). The code lives in `features/` by topic; this
 * module is the one import for all of it:
 *
 * - `feature-words.ts`: a feature's status, attention list and confirmations;
 * - `slice-words.ts`: a slice's status, what it waits on, its attempts and stop reasons, its card
 *   in the graph and the sentence at the top of its panel;
 * - `slice-panel-words.ts`: the slice panel's confirmations, edit checks, attempt reasons, artifacts;
 * - `feature-events.ts`: the timeline's events;
 * - `hook-output.ts`: hook output without its rule lines;
 * - `slice-graph-model.ts`: the slice graph's edges and layout input;
 * - `diff-text.ts`: diffs split per file.
 */
export {
  attentionItems,
  featureState,
  findPaddedSlice,
  isLiveRow,
  nowParts,
  pausedSummary,
  progressText,
  sentence,
  sortFeatures,
  stoppedReason,
  toastText,
  type AttentionItem,
  type NowPart,
} from './features/feature-words';
export {
  attemptDuration,
  attemptKindLabel,
  attemptResult,
  afterMerge,
  attemptsHint,
  attentionSlice,
  dependentsOf,
  failureWord,
  isActiveSlice,
  isStoppedSlice,
  needsAttention,
  parseDeps,
  reasonSummary,
  shortDuration,
  sliceCardLabel,
  sliceCardMeta,
  sliceCardState,
  sliceDuration,
  sliceStateSentence,
  sliceStatusLabel,
  sliceTone,
  specBody,
  sliceStateText,
  attemptStepLabel,
  mergeStep,
  REVIEW_WAIT,
  GATE_WORDS,
  GATE_FULL,
  APPROVAL_SLICE,
  APPROVAL_SLICE_HINT,
  waitVerb,
  waitingText,
  type CardIcon,
  type MergeStep,
  type SentenceContext,
  type StatusOf,
} from './features/slice-words';
export {
  acceptSummary,
  artifactKind,
  attemptReason,
  depMark,
  editDiff,
  REVIEW_ACTIONS,
  requestChangesBody,
  reviewActions,
  skipConsequences,
  type EditBody,
  type EditDraft,
  type ReviewAction,
} from './features/slice-panel-words';
export { eventLinksRun, eventSlices, eventText, eventTone, isNoiseEvent, type FeatureEventKind } from './features/feature-events';
export { cleanOutput, hookLines, isRuleLine, oneLine, verdictLine } from './features/hook-output';
export {
  EDGE_LABEL,
  EDGE_STATES,
  SLICE_CARD,
  buildSliceElk,
  roundedPath,
  sliceEdges,
  sliceGraphKey,
  type EdgeState,
  type SliceEdge,
} from './features/slice-graph-model';
export { diffStat, splitDiff, statText } from './features/diff-text';
