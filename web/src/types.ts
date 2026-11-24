// Frozen: new types are imported straight from './generated/<module>', not added here.
//
// The zerochat server API's JSON, under the names the web uses.
//
// Most shapes are generated from the Rust types that serialise them: web/src/generated/<module>.ts,
// one file per module of crates/zerochat/src (`make types`; never edit those by hand). This file
// re-exports them, adds what the server's handlers (server.rs) put on top of a Rust type, and
// describes the answers the handlers build as plain JSON (no Rust type yet).
//
// Every field is camelCase except where noted; optional fields are often `null` rather than absent.
// Two tools besides Zeroshot appear in the API:
// - pb, the ProgramBench runner: runs a benchmark (tasks × seeds) against Zeroshot graphs and
//   records the runs, scores and costs in its own database (the Benchmarks pages).
// - zsf, Zeroshot features: splits a feature into slices and runs each slice as a Zeroshot run
//   (the Features pages).

import type { RunListing as RunListingRow } from './generated/source';
import type { RunContext } from './generated/context';
import type { TargetKind } from './generated/discovery';
import type { Execution } from './generated/projection';
import type { RunView as ProjectedRunView } from './generated/projection';
import type { BatchCompare as BatchCompareRow, BatchDetail as BatchDetailRow, Cell, HarnessOptions as HarnessOptionsRow, Noise, TaskCompareRow as TaskCompareRowRust } from './generated/benchmarks';
import type { FeatureDetail as FeatureDetailRow } from './generated/features';
import type { Transcript } from './generated/transcript';
import type { BenchmarkCellState as CellState, CutShort as CutShortRust, FeatureAction, SliceAction } from './generated/shapes';

export type {
  AttemptResult,
  FailureClass,
  FeatureAction,
  Json,
  JsonObject,
  SliceAction,
  SliceStatus,
  WorkspaceSnapshot,
} from './generated/shapes';
export type { Usage } from './generated/pricing';
export type { RunContext } from './generated/context';
export type { TargetKind } from './generated/discovery';
export type {
  BackgroundAgent,
  ExecState,
  Execution,
  Failure,
  Feedback,
  GraphNode as GraphNodeInfo,
  LogLine,
  NodeStats,
  Pressure,
  Action as FailureAction,
} from './generated/projection';
export type {
  Item as TranscriptItem,
  ItemKind as TranscriptItemKind,
  Permission as ToolPermission,
  Tool,
  ToolResult,
  Transcript,
} from './generated/transcript';
export type {
  BatchSummary,
  Aggregate as BenchmarkAggregate,
  Limits,
  NodeAgg,
  NodeCompareRow,
  NodeCost,
  PriorAttempt,
  ReplayContext,
  RunScore,
  StepScore,
  SweepSummary,
  TaskAgg,
  TaskSet,
  TotalsRow,
} from './generated/benchmarks';
export type { HarnessInfo, HarnessNode, HarnessPreview } from './generated/harness';
export type {
  RunSlots,
  SweepAllocation,
  SweepMappingChange,
  SweepModelStat,
  SweepRecommendation,
  SweepReport,
  SweepSuggestion,
} from './generated/pb_exec';
export type {
  AttemptView,
  ChangeStat,
  FeatureChanges,
  FeatureRow,
  FileStat,
  CommitView as SliceCommit,
  SliceView,
  TaskView as SliceTask,
  WorktreeDiff,
} from './generated/features';

// ---------- runs ----------

/** A run in a target's list: the Rust `RunListing`, plus what the overview handler adds. */
export type RunListing = RunListingRow & {
  /** Computed by the same projection as the run page; absent while the run cannot be read. */
  summary?: RunSummary;
  /** Set when pb or zsf recognises this run: the sidebar's task-name label. */
  context?: RunContext;
};

/** A run's row summary (server.rs builds it from the run's projection). */
export type RunSummary = {
  status: string;
  /** 2-4 words: `credentials rejected`, `timed out after 45s`, `not accepted after 2 rounds`. */
  cause: string | null;
  failureClass: string | null;
  failureNode: string | null;
  startedAt: number | null;
  endedAt: number | null;
  durationMs: number | null;
  costUsd: number | null;
  /** Part of `costUsd` is estimated from tokens. */
  costEstimated?: boolean;
  resumedAs: string | null;
  resumedFrom: string | null;
  replayOf: string | null;
};

/** One target in the overview (server.rs): its runs, or why they cannot be read. */
export type TargetOverview = {
  id: string;
  name: string;
  kind: TargetKind;
  origin?: string | null;
  status: 'ok' | 'unreachable' | 'unsupported';
  error?: string;
  runs: RunListing[];
  capabilities?: string[];
};

/** `GET /api/overview` (server.rs). */
export type Overview = {
  targets: TargetOverview[];
  issue: string | null;
  zeroshot: { program: string; version: string | null };
  stateRoot: string;
  /** zsf has at least one feature (the Features section is shown). */
  features?: boolean;
  /** Features that need a person now (the nav badge). */
  featuresNeedYou?: number;
  /** `nav`: answered for `?view=nav` (targets without runs, for pages with no run list). */
  view?: 'full' | 'nav';
  now: number;
};

/** The run page's run: the Rust `RunView`, plus what the run handler (server.rs) adds. */
export type RunView = ProjectedRunView & {
  /** `pb`: `costUsd` is pb's own recorded cost for this benchmark run (its pricing table), and
   * Zeroshot's estimate from the same tokens is `zeroshotCostUsd`. */
  costSource?: 'pb';
  zeroshotCostUsd?: number | null;
  zeroshotCostEstimated?: boolean;
  capabilities: string[];
  targetKind: TargetKind;
  targetName: string;
  /** Set when pb or zsf recognises this run. */
  context?: RunContext | null;
};

/** `GET .../executions/{id}/transcript` (server.rs). */
export type TranscriptResponse = {
  execution: Execution;
  instructions: string | null;
  transcript: Transcript;
  /** Why a richer source (the provider's own session log) was not used. */
  sourceNote?: string | null;
};

export type Checkpoint = {
  checkpointId: string;
  sequence: number;
  node: string;
  mapIndices: number[];
  loopIterations: number[];
  createdAt: number;
};

/** A file changed in a run's workspace (workspace.rs, from git). */
export type ChangedFile = {
  path: string;
  status: string;
  additions: number | null;
  deletions: number | null;
  binary?: boolean;
  untracked?: boolean;
};

export type ChangesResponse = {
  files: ChangedFile[];
  workspace?: string;
  from?: string;
  to?: string | null;
  head?: string | null;
  /** How `from` was chosen: `source`, `snapshot`, `checked_out` or `execution`. */
  base?: string;
  note?: string;
};

/** The server's answer to a message for the agents. Zeroshot 16.1: `sent`, with who it goes to
 * (`node`: one agent node; `run`: every agent of the run) and Zeroshot's receipt (the steer id
 * and how many agent conversations had taken it in when it returned, usually 0). */
export type SteerResult = {
  status: 'sent' | 'delivered' | 'queued' | 'rejected' | string;
  reason?: string;
  scope?: 'node' | 'run';
  node?: string | null;
  steerId?: string;
  receivedExecutions?: number;
};

/** A replay request (`POST …/replay`, the Rust `ReplayBody`). Zeroshot only resumes a checkpoint
 * unchanged, so the server refuses `graph`, `runtime` and `initialInput`, and ignores `title`. */
export type ReplayBody = {
  checkpointId: string;
  graph?: unknown;
  runtime?: unknown;
  initialInput?: unknown;
  title?: string;
  validateOnly?: boolean;
};

/** The answer to a replay: `{valid: true, mode: 'resume'}` for `validateOnly`, else Zeroshot's own
 * `resume` output passed through (its `runId` and whatever else that Zeroshot reports). A refused
 * replay is an error response, not a result. */
export type ReplayResult = {
  valid?: boolean;
  mode?: 'resume';
  runId?: string;
  [key: string]: unknown;
};

// ---------- benchmarks (pb) ----------

// The Benchmarks pages show a value a newer pb may add as pb wrote it, and work with cells and
// options an older server leaves out (benchmarks-model.ts and its tests rely on both). The types
// below are the generated ones with exactly those fields widened.

/** One of `T`, or another string a newer pb may send (`string & {}` keeps `T`'s autocompletion). */
type OrNewer<T extends string> = T | (string & {});

export type BenchmarkCellState = OrNewer<CellState>;
export type CutShort = OrNewer<CutShortRust>;
export type NoiseVerdict = OrNewer<Noise>;

/** A batch cell: the Rust `Cell`, plus what benchmarks.rs adds to a running cell. */
export type BenchmarkCell = Omit<Cell, 'state' | 'cutShort' | 'capMinutes' | 'capSteps' | 'startedAt'> & {
  state: BenchmarkCellState;
  cutShort?: CutShort | null;
  capMinutes?: number | null;
  capSteps?: number | null;
  startedAt?: number | null;
  /** Running cells only: the nodes the agent is in now (empty once the agent finished and pb is
   * wrapping up), and a live estimate of its spend so far, priced with pb's own price table (pb
   * records the real cost when the agent ends). */
  liveNodes?: string[];
  liveCostUsd?: number;
  /** Models pb has no price for: their tokens are not in `liveCostUsd`. */
  liveCostUnpriced?: string[];
};

export type BatchDetail = Omit<BatchDetailRow, 'cells' | 'starting'> & { cells: BenchmarkCell[]; starting?: boolean };

export type TaskCompareRow = Omit<TaskCompareRowRust, 'noise'> & { noise: NoiseVerdict };
export type BatchCompare = Omit<BatchCompareRow, 'tasks'> & { tasks: TaskCompareRow[] };

/** The Start form's options; the form starts from defaults without the server's lists. */
export type HarnessOptions = Omit<HarnessOptionsRow, 'models' | 'defaultGraph' | 'defaultRuntime' | 'harnessFiles' | 'maxConcurrentRuns'> &
  Partial<Pick<HarnessOptionsRow, 'models' | 'defaultGraph' | 'defaultRuntime' | 'harnessFiles' | 'maxConcurrentRuns'>>;

/** What the Start form asks pb to run (`harness` picks the graph and runtime). */
export type Harness = { kind: 'default' } | { kind: 'variant'; name: string } | { kind: 'custom'; graph: string; runtime: string };

export type StartBatchRequest = {
  tasks: string;
  seeds: number;
  label: string;
  harness: Harness;
  model?: string;
  maxCostUsd?: number;
  maxMinutes?: number;
  maxSteps?: number;
  parallel?: number;
};

// ---------- features (zsf) ----------

/** A feature's page: the Rust `FeatureDetail`, plus the server's clock. */
export type FeatureDetail = FeatureDetailRow & { now: number };

/** One line of zsf's `events.jsonl` (zsf's own `Event`, so `run_id` is snake_case), and where its
 * run can be opened. */
export type FeatureEvent = {
  ts: number;
  slice?: string;
  attempt?: number;
  kind: string;
  run_id?: string;
  detail?: string;
  runTarget: string | null;
};

export type FeatureActionBody = {
  action: FeatureAction | SliceAction;
  notes?: string;
  fresh?: boolean;
  maxParallel?: number;
  title?: string;
  deps?: string[];
  spec?: string;
};
