# Web model goldens

Every exported function (and exported constant table) of the web models is pinned byte for byte by a
golden test: `src/<module>.golden.test.ts` renders the output as JSON (keys sorted, two-space
indent) and compares it with `src/testdata/golden/<golden>.json`. The inputs are the Rust side's
snapshots of the server's JSON API, `crates/zerochat/testdata/golden/api/**` (paths below are
relative to that directory), plus tables of edge inputs written in the test.

- Run: `npm --prefix web test`. Accept an intended change: `UPDATE_GOLDENS=1 npm --prefix web test`,
  then review the diff. A mismatch prints the first differing lines and writes
  `<golden>.actual.json` (gitignored).
- The environment is pinned in `src/testdata/golden.ts`: `TZ=UTC`, `Date.now()` = 1795003600000 (the
  snapshots' `now`), and locale calls made without a locale (`toLocale*String`, `localeCompare`)
  use `en-US`. Time formatting still follows the ICU data of the Node version that runs the tests.
- Objects a model passes through untouched are written as `"<ref:path>"` (the input object it is);
  `undefined`, `NaN`, `Map` and `Set` are spelled out (`"<undefined>"`, `{"<Map>": [...]}`, ...).
- Summarised outputs (lossless against their input): `transcript-model/chatEntries` writes each
  entry as `key = index` (each item checked to be the transcript's own item at that index);
  `replay-model/applyEdits` writes the edited graph/runtime as their changes against the submitted
  graph/runtime; `features-model/splitDiff` writes each part as its `[start, end)` offsets in the
  diff; `transcript-model/editPreview` pins the >250k-cell fallback of a generated 600-line edit
  by its shape.

Not covered: none; every row below has a golden.

| module | export | golden test | fixture(s) |
|---|---|---|---|
| graph-model | `humanId` | `graph-model.golden.test.ts`: `graph-model/humanId.json` | node names / edge labels of `api/runs/**/run.json` (all 68) + table |
| graph-model | `guardNames` | `graph-model.golden.test.ts`: `graph-model/guardNames.json` | every `when` in `api/runs/**/run.json` (all 68) + table |
| graph-model | `conditionLabel` | `graph-model.golden.test.ts`: `graph-model/conditionLabel.json` | every `when` in `api/runs/**/run.json` (all 68) + table × local sets |
| graph-model | `shortLabel` | `graph-model.golden.test.ts`: `graph-model/shortLabel.json` | node names / edge labels of `api/runs/**/run.json` (all 68) + table |
| graph-model | `buildFlowModel` | `graph-model.golden.test.ts`: `graph-model/buildFlowModel.json`, `graph-model/buildFlowModel-odd.json` | `api/runs/**/run.json` (all 68) + odd graphs |
| graph-model | `showEveryModel` | `graph-model.golden.test.ts`: `graph-model/showEveryModel.json` | `api/runs/**/run.json` (all 68) |
| graph-model | `commonModel` | `graph-model.golden.test.ts`: `graph-model/commonModel.json` | `api/runs/**/run.json` (all 68) |
| graph-model | `pickReachedFail` | `graph-model.golden.test.ts`: `graph-model/pickReachedFail.json` | `api/runs/**/run.json` (all 68) (guards map synthesised: the internal one is not exported) |
| graph-model | `itemSize` | `graph-model.golden.test.ts`: `graph-model/itemSize.json` | `api/runs/**/run.json` (all 68) |
| graph-model | `labelSize` | `graph-model.golden.test.ts`: `graph-model/labelSize.json` | node names / edge labels of `api/runs/**/run.json` (all 68) + table |
| graph-model | `buildElkGraph` | `graph-model.golden.test.ts`: `graph-model/buildElkGraph.json` | `api/runs/**/run.json` (all 68) |
| graph-model | `readElkLayout` | `graph-model.golden.test.ts`: `graph-model/readElkLayout.json` | `api/runs/**/run.json` (all 68) laid out by ELK (RIGHT, DOWN) + hand tree |
| features-model | `sliceTone` | `features-model.golden.test.ts`: `features-model/sliceTone.json` | slices of `api/features/*/detail.json` + table |
| features-model | `sliceStatusLabel` | `features-model.golden.test.ts`: `features-model/sliceStatusLabel.json` | slices of `api/features/*/detail.json` + table |
| features-model | `isActiveSlice` | `features-model.golden.test.ts`: `features-model/isActiveSlice.json` | slices of `api/features/*/detail.json` + table |
| features-model | `shortDuration` | `features-model.golden.test.ts`: `features-model/shortDuration.json` | slices of `api/features/*/detail.json` + table |
| features-model | `waitingText` | `features-model.golden.test.ts`: `features-model/waitingText.json` | slices of `api/features/*/detail.json` + table |
| features-model | `failureWord` | `features-model.golden.test.ts`: `features-model/failureWord.json` | slices of `api/features/*/detail.json` + table |
| features-model | `pausedSummary` | `features-model.golden.test.ts`: `features-model/pausedSummary.json` | `api/features/list.json`, `api/features/*/detail.json` + table |
| features-model | `nowParts` | `features-model.golden.test.ts`: `features-model/nowParts.json`, `features-model/rows-table.json` | `api/features/list.json`, `api/features/*/detail.json` + synthetic rows |
| features-model | `featureState` | `features-model.golden.test.ts`: `features-model/featureState.json`, `features-model/rows-table.json` | `api/features/list.json`, `api/features/*/detail.json` + synthetic rows |
| features-model | `sortFeatures` | `features-model.golden.test.ts`: `features-model/sortFeatures.json` | `api/features/list.json` + table |
| features-model | `progressText` | `features-model.golden.test.ts`: `features-model/progressText.json`, `features-model/rows-table.json` | `api/features/list.json`, `api/features/*/detail.json` + synthetic rows |
| features-model | `attemptKindLabel` | `features-model.golden.test.ts`: `features-model/attemptKindLabel.json` | slices of `api/features/*/detail.json` + table |
| features-model | `attemptResult` | `features-model.golden.test.ts`: `features-model/attemptResult.json` | slices of `api/features/*/detail.json` + table |
| features-model | `sliceDuration` | `features-model.golden.test.ts`: `features-model/sliceDuration.json` | slices of `api/features/*/detail.json` + table |
| features-model | `attemptDuration` | `features-model.golden.test.ts`: `features-model/attemptDuration.json` | slices of `api/features/*/detail.json` + table |
| features-model | `verdictLine` | `features-model.golden.test.ts`: `features-model/verdictLine.json` | feature reasons/notes/tasks/artifacts/event details + table |
| features-model | `reasonSummary` | `features-model.golden.test.ts`: `features-model/reasonSummary.json` | feature reasons/notes/tasks/artifacts/event details + table |
| features-model | `isNoiseEvent` | `features-model.golden.test.ts`: `features-model/isNoiseEvent.json`, `features-model/events-table.json` | `api/features/**/events*.json` + table |
| features-model | `eventText` | `features-model.golden.test.ts`: `features-model/eventText.json`, `features-model/events-table.json` | `api/features/**/events*.json` + table |
| features-model | `eventTone` | `features-model.golden.test.ts`: `features-model/eventTone.json` | kinds in `api/features/**/events*.json` |
| features-model | `eventSlices` | `features-model.golden.test.ts`: `features-model/eventSlices.json` | `api/features/**/events*.json` + table |
| features-model | `oneLine` | `features-model.golden.test.ts`: `features-model/oneLine.json` | feature reasons/notes/tasks/artifacts/event details + table |
| features-model | `specBody` | `features-model.golden.test.ts`: `features-model/specBody.json` | slices of `api/features/*/detail.json` + table |
| features-model | `SLICE_CARD` (const) | `features-model.golden.test.ts`: `features-model/SLICE_CARD.json` | none |
| features-model | `sliceEdges` | `features-model.golden.test.ts`: `features-model/sliceEdges.json` | `api/features/*/detail.json` slices + table |
| features-model | `sliceGraphKey` | `features-model.golden.test.ts`: `features-model/sliceGraphKey.json` | `api/features/*/detail.json` slices + table |
| features-model | `buildSliceElk` | `features-model.golden.test.ts`: `features-model/buildSliceElk.json` | `api/features/*/detail.json` slices + table |
| features-model | `roundedPath` | `features-model.golden.test.ts`: `features-model/roundedPath.json` | ELK edge routes of `api/features/*/detail.json` slices + table |
| features-model | `parseDeps` | `features-model.golden.test.ts`: `features-model/parseDeps.json` | table |
| features-model | `splitDiff` | `features-model.golden.test.ts`: `features-model/splitDiff.json` | `api/features/**` changes/commit/task/worktree diffs + table |
| features-model | `waitVerb` | `features-model.golden.test.ts`: `features-model/waitVerb.json` | table |
| features-model | `cleanOutput` | `features-model.golden.test.ts`: `features-model/cleanOutput.json` | feature reasons/notes/tasks/artifacts/event details + table |
| features-model | `statText` | `features-model.golden.test.ts`: `features-model/statText.json` | `api/features/*/changes*.json`, slice worktree stats + table |
| benchmarks-model | `stateLabel` | `benchmarks-model.golden.test.ts`: `benchmarks-model/stateLabel.json` | `api/benchmarks/batch/*.json`, `api/benchmarks/score/*.json` + table |
| benchmarks-model | `cellStateTone` | `benchmarks-model.golden.test.ts`: `benchmarks-model/cellStateTone.json` | `api/benchmarks/batch/*.json`, `api/benchmarks/score/*.json` + table |
| benchmarks-model | `cutShortBadge` | `benchmarks-model.golden.test.ts`: `benchmarks-model/cutShortBadge.json` | `api/benchmarks/batch/*.json`, `api/benchmarks/score/*.json` + table |
| benchmarks-model | `scoreColor` | `benchmarks-model.golden.test.ts`: `benchmarks-model/scoreColor.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `scoreText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/scoreText.json` | `api/benchmarks/batch/*.json`, `api/benchmarks/score/*.json` + table |
| benchmarks-model | `aggregateText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/aggregateText.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `cellTitle` | `benchmarks-model.golden.test.ts`: `benchmarks-model/cellTitle.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `seedHeader` | `benchmarks-model.golden.test.ts`: `benchmarks-model/seedHeader.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `replaySegmentText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/replaySegmentText.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `replayModelText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/replayModelText.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `liveNodeText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/liveNodeText.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `cellKey` | `benchmarks-model.golden.test.ts`: `benchmarks-model/cellKey.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `cellMap` | `benchmarks-model.golden.test.ts`: `benchmarks-model/cellMap.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `aggregateByKey` | `benchmarks-model.golden.test.ts`: `benchmarks-model/aggregateByKey.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `batchIsLive` | `benchmarks-model.golden.test.ts`: `benchmarks-model/batchIsLive.json` | `api/benchmarks/batches.json` + table |
| benchmarks-model | `retriedText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/retriedText.json` | `api/benchmarks/batches.json`, `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `batchMaxSpend` | `benchmarks-model.golden.test.ts`: `benchmarks-model/batchMaxSpend.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `detailIsLive` | `benchmarks-model.golden.test.ts`: `benchmarks-model/detailIsLive.json` | `api/benchmarks/batch/*.json` + table |
| benchmarks-model | `START_PATIENCE_MS` (const) | `benchmarks-model.golden.test.ts`: `benchmarks-model/constants.json` | none |
| benchmarks-model | `noteStarted` | `benchmarks-model.golden.test.ts`: `benchmarks-model/batchStartPhase.json`, `benchmarks-model/startedRecently.json` | `api/benchmarks/batch/missing.json` (404); in-memory only (no `sessionStorage` under node) |
| benchmarks-model | `startedRecently` | `benchmarks-model.golden.test.ts`: `benchmarks-model/startedRecently.json` | ids noted by `noteStarted` |
| benchmarks-model | `batchStartPhase` | `benchmarks-model.golden.test.ts`: `benchmarks-model/batchStartPhase.json` | `api/benchmarks/batch/*.json` incl. `missing.json` (404) + table |
| benchmarks-model | `noiseText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/noiseText.json` | table of verdicts |
| benchmarks-model | `noiseTone` | `benchmarks-model.golden.test.ts`: `benchmarks-model/noiseTone.json` | table of verdicts |
| benchmarks-model | `deltaTone` | `benchmarks-model.golden.test.ts`: `benchmarks-model/deltaTone.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `totalsVerdict` | `benchmarks-model.golden.test.ts`: `benchmarks-model/totalsVerdict.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `signedPp` | `benchmarks-model.golden.test.ts`: `benchmarks-model/signed.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `signedUsd` | `benchmarks-model.golden.test.ts`: `benchmarks-model/signed.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `signedDuration` | `benchmarks-model.golden.test.ts`: `benchmarks-model/signed.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `taskAggText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/taskAggText.json` | `api/benchmarks/compare/*.json` + table |
| benchmarks-model | `resolveTasks` | `benchmarks-model.golden.test.ts`: `benchmarks-model/resolveTasks.json` | task sets = `defaultHarnessOptions()` + each batch's tasks (`api/benchmarks/start-options.json` is a 500) |
| benchmarks-model | `tasksArg` | `benchmarks-model.golden.test.ts`: `benchmarks-model/tasksArg.json` | same selections as `resolveTasks` |
| benchmarks-model | `maxSpend` | `benchmarks-model.golden.test.ts`: `benchmarks-model/maxSpend.json` | table |
| benchmarks-model | `validateStart` | `benchmarks-model.golden.test.ts`: `benchmarks-model/validateStart.json` | table of form values |
| benchmarks-model | `START_FIELD_LABEL` (const) | `benchmarks-model.golden.test.ts`: `benchmarks-model/constants.json` | none |
| benchmarks-model | `setsIncluding` | `benchmarks-model.golden.test.ts`: `benchmarks-model/setsIncluding.json` | task sets from `api/benchmarks/batch/*.json` |
| benchmarks-model | `harnessSegments` | `benchmarks-model.golden.test.ts`: `benchmarks-model/harnessSegments.json` | `api/benchmarks/batch/*.json` harnesses, `api/benchmarks/harness-preview/*.json` |
| benchmarks-model | `nodeModelText` | `benchmarks-model.golden.test.ts`: `benchmarks-model/nodeModelText.json` | `api/benchmarks/batch/*.json` harnesses, `api/benchmarks/harness-preview/*.json` + table |
| benchmarks-model | `DEFAULT_MAX_CONCURRENT_RUNS` (const) | `benchmarks-model.golden.test.ts`: `benchmarks-model/constants.json` | none |
| benchmarks-model | `defaultParallel` | `benchmarks-model.golden.test.ts`: `benchmarks-model/defaultParallel.json` | table |
| benchmarks-model | `concurrencyNote` | `benchmarks-model.golden.test.ts`: `benchmarks-model/concurrencyNote.json` | running batches of `api/benchmarks/batches.json` × slots table (`api/benchmarks/slots.json` is a 400) |
| benchmarks-model | `defaultHarnessOptions` | `benchmarks-model.golden.test.ts`: `benchmarks-model/defaultHarnessOptions.json` | none |
| transcript-model | `toolSummary` | `transcript-model.golden.test.ts`: `transcript-model/toolSummary.json` | every tool call in `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `editPreview` | `transcript-model.golden.test.ts`: `transcript-model/editPreview.json` | every tool call in `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `chatEntries` | `transcript-model.golden.test.ts`: `transcript-model/chatEntries.json`, `transcript-model/chatEntries-edge.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + edge sequences |
| transcript-model | `visibleRange` | `transcript-model.golden.test.ts`: `transcript-model/visibleRange.json` | table |
| transcript-model | `CHAT_PAGE` (const) | `transcript-model.golden.test.ts`: `transcript-model/constants.json` | none |
| transcript-model | `lastThinkingIndex` | `transcript-model.golden.test.ts`: `transcript-model/lastThinkingIndex.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `synthesizedPrompt` | `transcript-model.golden.test.ts`: `transcript-model/synthesizedPrompt.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `isNotice` | `transcript-model.golden.test.ts`: `transcript-model/isNotice.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `exitNote` | `transcript-model.golden.test.ts`: `transcript-model/exitNote.json` | every tool call in `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `deniedPermission` | `transcript-model.golden.test.ts`: `transcript-model/deniedPermission.json` | every tool call in `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `hasNativePrompt` | `transcript-model.golden.test.ts`: `transcript-model/hasNativePrompt.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `transcriptStats` | `transcript-model.golden.test.ts`: `transcript-model/transcriptStats.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `splitZeroshotPrompt` | `transcript-model.golden.test.ts`: `transcript-model/splitZeroshotPrompt.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `firstLine` | `transcript-model.golden.test.ts`: `transcript-model/firstLine.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `commonDir` | `transcript-model.golden.test.ts`: `transcript-model/commonDir.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `relativeTo` | `transcript-model.golden.test.ts`: `transcript-model/relativeTo.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `isResultTool` | `transcript-model.golden.test.ts`: `transcript-model/isResultTool.json` | every tool call in `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `lastSeen` | `transcript-model.golden.test.ts`: `transcript-model/lastSeen.json` | `api/runs/**/transcript-*.json` (all, incl. `-ledger`) + table |
| transcript-model | `IDLE_AFTER_MS` (const) | `transcript-model.golden.test.ts`: `transcript-model/constants.json` | none |
| replay-model | `AGENT_KINDS` (const) | `replay-model.golden.test.ts`: `replay-model/AGENT_KINDS.json` | none |
| replay-model | `ancestors` | `replay-model.golden.test.ts`: `replay-model/ancestors.json` | `api/runs/**/run.json` (all 68) |
| replay-model | `checkpointIteration` | `replay-model.golden.test.ts`: `replay-model/checkpointIteration.json` | table |
| replay-model | `preselectCheckpoint` | `replay-model.golden.test.ts`: `replay-model/preselectCheckpoint.json` | `api/runs/**/run.json` (all 68) (checkpoints synthesised, one per execution: the snapshots have no checkpoint listing) |
| replay-model | `editableNodes` | `replay-model.golden.test.ts`: `replay-model/editableNodes.json` | `api/runs/**/run.json` (all 68) |
| replay-model | `positiveInt` | `replay-model.golden.test.ts`: `replay-model/positiveInt.json` | table |
| replay-model | `submissionGraph` | `replay-model.golden.test.ts`: `replay-model/submissionGraph.json` | `api/runs/**/run.json` (all 68) |
| replay-model | `applyEdits` | `replay-model.golden.test.ts`: `replay-model/applyEdits.json` | `api/runs/**/run.json` (all 68) |
| replay-model | `parseJsonField` | `replay-model.golden.test.ts`: `replay-model/parseJsonField.json` | `api/runs/**/run.json` (all 68) runtime/initialInput + table |
| replay-model | `validationSummary` | `replay-model.golden.test.ts`: `replay-model/validationSummary.json` | `api/post/*.json` + table |
| replay-model | `validationLines` | `replay-model.golden.test.ts`: `replay-model/validationLines.json` | `api/post/*.json` + table |
| status | `listingStatus` | `status.golden.test.ts`: `status/listingStatus.json` | `api/overview.json` + edge listings |
| status | `runStatus` | `status.golden.test.ts`: `status/runStatus.json` | `api/runs/**/run.json` (all 68), `api/overview.json`, `api/overview-nav.json` |
| status | `runStatusInfo` (const) | `status.golden.test.ts`: `status/runStatusInfo.json` | none |
| status | `isLive` | `status.golden.test.ts`: `status/isLive.json` | every RunStatus |
| status | `isWatchable` | `status.golden.test.ts`: `status/isWatchable.json` | every RunStatus |
| status | `sortRuns` | `status.golden.test.ts`: `status/sortRuns.json` | `api/overview.json` |
| status | `statusGroup` | `status.golden.test.ts`: `status/statusGroup.json` | every RunStatus |
| status | `matchesFilter` | `status.golden.test.ts`: `status/matchesFilter.json` | `api/overview.json` × filter words |
| status | `runSubtitle` | `status.golden.test.ts`: `status/runSubtitle.json` | `api/overview.json` |
| status | `duplicateTitleSuffixes` | `status.golden.test.ts`: `status/duplicateTitleSuffixes.json` | `api/overview.json` |
| status | `idTail` | `status.golden.test.ts`: `status/idTail.json` | `api/overview.json` + table |
| status | `mergeOverview` | `status.golden.test.ts`: `status/mergeOverview.json` | `api/overview.json`, `api/overview-nav.json`, an unreadable variant |
| status | `overviewDelay` | `status.golden.test.ts`: `status/overviewDelay.json` | every mode × hidden |
| status | `orderTargets` | `status.golden.test.ts`: `status/orderTargets.json` | `api/overview.json`, `api/overview-nav.json` |
| status | `execStateInfo` (const) | `status.golden.test.ts`: `status/execStateInfo.json` | none |
| status | `nodeState` | `status.golden.test.ts`: `status/nodeState.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| status | `stateMark` | `status.golden.test.ts`: `status/stateMark.json` | every NodeState |
| status | `verdictText` | `status.golden.test.ts`: `status/verdictText.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| status | `diagnosticText` | `status.golden.test.ts`: `status/diagnosticText.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| status | `executionLabel` | `status.golden.test.ts`: `status/executionLabel.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| status | `positionText` | `status.golden.test.ts`: `status/positionText.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| status | `failureClassInfo` (const) | `status.golden.test.ts`: `status/failureClassInfo.json` | none |
| status | `failureClass` | `status.golden.test.ts`: `status/failureClass.json` | `api/runs/**/run.json` (all 68), `api/overview.json`, `api/overview-nav.json` |
| status | `missingData` | `status.golden.test.ts`: `status/missingData.json` | `api/runs/**/run.json` (all 68), `api/overview.json`, `api/overview-nav.json` |
| status | `observeOnly` | `status.golden.test.ts`: `status/observeOnly.json` | `api/runs/**/run.json` (all 68), `api/overview.json`, `api/overview-nav.json` |
| status | `observeOnlyNote` | `status.golden.test.ts`: `status/observeOnlyNote.json` | `api/runs/**/run.json` (all 68), `api/overview.json`, `api/overview-nav.json` |
| status | `stepExecution` | `status.golden.test.ts`: `status/stepExecution.json` | every execution in `api/runs/**/run.json` (all 68) + table |
| route | `parseRoute` | `route.golden.test.ts`: `route/parseRoute.json` | table of hashes |
| route | `formatRoute` | `route.golden.test.ts`: `route/formatRoute.json` | parsed hashes (round trip) + hand-made routes |
| route | `runHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksBatchHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `featuresHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `featureHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksCompareHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksStartHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksSweepsHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `benchmarksSweepReportHref` | `route.golden.test.ts`: `route/hrefs.json` | table of names |
| route | `nodeHref` | `route.golden.test.ts`: `route/hrefs.json`, `route/visitOf.json` | `api/runs/**/run.json` (all 68) + table |
| route | `visitOf` | `route.golden.test.ts`: `route/visitOf.json` | `api/runs/**/run.json` (all 68) + table |
| route | `navigate` | `route.golden.test.ts`: `route/navigate.json` | table (with `history`/`location`/`window` stubbed) |
| route | `resolveVisit` | `route.golden.test.ts`: `route/resolveVisit.json` | `api/runs/**/run.json` (all 68) × routes per execution |
| format | `duration` | `format.golden.test.ts`: `format/duration.json` | `api/runs/**/run.json` (all 68) + table |
| format | `relativeAge` | `format.golden.test.ts`: `format/relativeAge.json` | `api/runs/**/run.json` (all 68) + table |
| format | `compact` | `format.golden.test.ts`: `format/compact.json` | `api/runs/**/run.json` (all 68) + table |
| format | `usd` | `format.golden.test.ts`: `format/usd.json` | `api/runs/**/run.json` (all 68) + table |
| format | `totalTokens` | `format.golden.test.ts`: `format/totalTokens.json` | `api/runs/**/run.json` (all 68) + table |
| format | `usageBreakdown` | `format.golden.test.ts`: `format/usageBreakdown.json` | `api/runs/**/run.json` (all 68) + table |
| format | `clockTime` | `format.golden.test.ts`: `format/clockTime.json` | `api/runs/**/run.json` (all 68) + table |
| format | `fullTime` | `format.golden.test.ts`: `format/fullTime.json` | `api/runs/**/run.json` (all 68) + table |
| format | `offsetText` | `format.golden.test.ts`: `format/offsetText.json` | `api/runs/**/run.json` (all 68) + table |
| format | `shortId` | `format.golden.test.ts`: `format/shortId.json` | `api/runs/**/run.json` (all 68) + table |
| format | `shortModel` | `format.golden.test.ts`: `format/shortModel.json` | `api/runs/**/run.json` (all 68) + table |
| format | `plural` | `format.golden.test.ts`: `format/plural.json` | `api/runs/**/run.json` (all 68) + table |
| format | `prettyJson` | `format.golden.test.ts`: `format/prettyJson.json` | `api/runs/**/run.json` (all 68) + table |
| format | `truncate` | `format.golden.test.ts`: `format/truncate.json` | `api/runs/**/run.json` (all 68) + table |
| format | `timeOfDay` | `format.golden.test.ts`: `format/timeOfDay.json` | `api/runs/**/run.json` (all 68) + table |
| format | `untilText` | `format.golden.test.ts`: `format/untilText.json` | `api/runs/**/run.json` (all 68) + table |
| format | `tokenParts` | `format.golden.test.ts`: `format/tokenParts.json` | `api/runs/**/run.json` (all 68) + table |
| format | `costText` | `format.golden.test.ts`: `format/costText.json` | `api/runs/**/run.json` (all 68) + table |
| markdown | `parseMarkdown` | `markdown.golden.test.ts`: `markdown/parseMarkdown-table.json`, `markdown/parseMarkdown-snapshots.json` | transcript texts in `api/runs/**/transcript-*.json` (all, incl. `-ledger`), feature specs/tasks/artifacts + table |
| markdown | `parseInline` | `markdown.golden.test.ts`: `markdown/parseInline-table.json`, `markdown/parseInline-snapshots.json` | transcript texts in `api/runs/**/transcript-*.json` (all, incl. `-ledger`), feature specs/tasks/artifacts + table (first lines) |
| diff | `parseDiff` | `diff.golden.test.ts`: `diff/parseDiff-snapshots.json`, `diff/parseDiff-table.json` | `api/workspace/*diff*.json`, feature changes/commit/task/worktree diffs + table |
| diff | `fileStatusLabel` (const) | `diff.golden.test.ts`: `diff/fileStatusLabel.json` | statuses in `api/workspace/*changes*.json` |
| diff | `buildTree` | `diff.golden.test.ts`: `diff/buildTree.json` | `api/workspace/*tree*.json`, `*changes*.json`, feature changes + table |
| diff | `isArtefact` | `diff.golden.test.ts`: `diff/isArtefact.json` | every path of the trees/changes + table |
