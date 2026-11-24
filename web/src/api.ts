import type {
  BatchCompare,
  FeatureActionBody,
  FeatureChanges,
  WorktreeDiff,
  FeatureDetail,
  FeatureEvent,
  FeatureRow,
  SliceCommit,
  SliceTask,
  BatchDetail,
  BatchSummary,
  ChangesResponse,
  Checkpoint,
  HarnessOptions,
  RunSlots,
  HarnessPreview,
  Overview,
  RunScore,
  RunView,
  StartBatchRequest,
  ReplayBody,
  ReplayResult,
  SteerResult,
  SweepReport,
  SweepSummary,
  TranscriptResponse,
} from './types';
import { queryString } from './url';

/** An API failure with the server's own human message (`{code, message}`) when it sent one. */
export class ApiError extends Error {
  code: string;
  status: number;
  constructor(code: string, message: string, status: number) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

type Fetch = typeof fetch;
let fetcher: Fetch = (...args) => fetch(...args);
/** Tests swap the transport. */
export function setFetch(next: Fetch) {
  fetcher = next;
}

/** Turns any response into a value or an ApiError whose message a person can act on. */
export async function readResponse<T>(response: Response, what: string): Promise<T> {
  let body: unknown;
  const text = await response.text();
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = undefined;
    }
  }
  if (!response.ok) {
    const b = body as { code?: unknown; message?: unknown } | undefined;
    if (b && typeof b.message === 'string') {
      throw new ApiError(typeof b.code === 'string' ? b.code : 'error', b.message, response.status);
    }
    throw new ApiError(`http_${response.status}`, `Could not ${what} (${response.status}).`, response.status);
  }
  if (body === undefined) {
    throw new ApiError('bad_response', `Could not ${what}: the server sent something that is not JSON.`, response.status);
  }
  return body as T;
}

/** GET (or `init`) `path` as JSON; failures are `ApiError`s saying what could not be done (`what`). */
export async function request<T>(path: string, what: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetcher(path, init);
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
    // The browser's own wording ("Failed to fetch", "NetworkError…") is jargon, not a reason.
    throw new ApiError('network', `Could not ${what}: the zerochat server did not answer.`, 0);
  }
  return readResponse<T>(response, what);
}

/** POST `body` as JSON to `path` (see `request`). */
export function post<T>(path: string, body: unknown, what: string): Promise<T> {
  return request<T>(path, what, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

/** An action id in words for an error message: `max_parallel` -> `max parallel`. */
function actionWords(action: FeatureActionBody['action']): string {
  return action.replace('_', ' ');
}

// ---------- runs ----------

export function runPath(target: string, run: string) {
  return `/api/runs/${encodeURIComponent(target)}/${encodeURIComponent(run)}`;
}

type Range = { from?: string | null; to?: string | null };

export const runsApi = {
  overview: (signal?: AbortSignal) => request<Overview>('/api/overview', 'list runs', { signal }),
  /** Targets and the Features badge only, no runs: for pages that show no run list. */
  overviewNav: (signal?: AbortSignal) => request<Overview>('/api/overview?view=nav', 'load the navigation', { signal }),
  run: (target: string, run: string, signal?: AbortSignal) => request<RunView>(runPath(target, run), 'load this run', { signal }),
  transcript: (target: string, run: string, execution: number, signal?: AbortSignal) =>
    request<TranscriptResponse>(`${runPath(target, run)}/transcript/${execution}`, "load this node's conversation", { signal }),
  checkpoints: (target: string, run: string, signal?: AbortSignal) => request<{ checkpoints: Checkpoint[] }>(`${runPath(target, run)}/checkpoints`, 'list checkpoints', { signal }),

  // The run's workspace: changed files, diffs, file contents.
  changes: (target: string, run: string, range: Range, signal?: AbortSignal) => request<ChangesResponse>(`${runPath(target, run)}/changes${queryString(range)}`, 'list changed files', { signal }),
  diff: (target: string, run: string, range: Range & { path?: string | null }, signal?: AbortSignal) =>
    request<{ diff: string }>(`${runPath(target, run)}/diff${queryString(range)}`, 'load the diff', { signal }),
  file: (target: string, run: string, params: { commit?: string | null; path: string }, signal?: AbortSignal) =>
    request<{ path: string; commit: string | null; content: string }>(`${runPath(target, run)}/file${queryString(params)}`, 'load the file', { signal }),
  tree: (target: string, run: string, commit: string | null | undefined, signal?: AbortSignal) =>
    request<{ paths: string[]; commit: string | null }>(`${runPath(target, run)}/tree${queryString({ commit })}`, 'list files', { signal }),

  // Actions.
  steer: (target: string, run: string, execution: number, message: string) => post<SteerResult>(`${runPath(target, run)}/steer`, { execution, message }, 'send the message'),
  stop: (target: string, run: string) => post<Record<string, unknown>>(`${runPath(target, run)}/stop`, {}, 'stop the run'),
  resume: (target: string, run: string, checkpointId?: string) => post<Record<string, unknown>>(`${runPath(target, run)}/resume`, checkpointId ? { checkpointId } : {}, 'resume the run'),
  replay: (target: string, run: string, body: ReplayBody) => post<ReplayResult>(`${runPath(target, run)}/replay`, body, body.validateOnly ? 'validate the replay' : 'start the replay'),
};

// ---------- features (zsf) ----------

function featurePath(name: string) {
  return `/api/features/${encodeURIComponent(name)}`;
}

function slicePath(name: string, slice: string) {
  return `${featurePath(name)}/slices/${encodeURIComponent(slice)}`;
}

type ActionAnswer = { ok: boolean; message: string };

export const featuresApi = {
  features: (signal?: AbortSignal) => request<{ features: FeatureRow[]; now: number }>('/api/features', 'list features', { signal }),
  feature: (name: string, signal?: AbortSignal) => request<FeatureDetail>(featurePath(name), 'load this feature', { signal }),
  featureEvents: (name: string, slice: string | null, signal?: AbortSignal) =>
    request<{ events: FeatureEvent[] }>(`${featurePath(name)}/events${queryString({ slice })}`, "load the feature's events", { signal }),
  featureChanges: (name: string, withDiff: boolean, signal?: AbortSignal) =>
    request<FeatureChanges>(`${featurePath(name)}/changes${withDiff ? '?diff=true' : ''}`, "load the feature's changes", { signal }),
  featureAction: (name: string, body: FeatureActionBody) => post<ActionAnswer>(`${featurePath(name)}/actions`, body, `${actionWords(body.action)} ${name}`),

  sliceTask: (name: string, slice: string, attempt: number, signal?: AbortSignal) => request<SliceTask>(`${slicePath(name, slice)}/task/${attempt}`, 'load the task', { signal }),
  sliceCommit: (name: string, slice: string, signal?: AbortSignal) => request<SliceCommit>(`${slicePath(name, slice)}/commit`, 'load the merged commit', { signal }),
  sliceWorktree: (name: string, slice: string, signal?: AbortSignal) => request<WorktreeDiff>(`${slicePath(name, slice)}/worktree`, "load the worktree's changes", { signal }),
  sliceArtifact: (name: string, slice: string, file: string, signal?: AbortSignal) =>
    request<{ path: string; content: string }>(`${slicePath(name, slice)}/artifacts/${file.split('/').map(encodeURIComponent).join('/')}`, 'load the artifact', { signal }),
  sliceAction: (name: string, slice: string, body: FeatureActionBody) => post<ActionAnswer>(`${slicePath(name, slice)}/actions`, body, `${actionWords(body.action)} ${slice}`),
};

// ---------- benchmarks (pb) ----------

function batchPath(id: string) {
  return `/api/benchmarks/batches/${encodeURIComponent(id)}`;
}

type SweepFilter = { ids: string[]; label?: string; task?: string };

export const benchmarksApi = {
  benchmarkBatches: (signal?: AbortSignal) => request<{ batches: BatchSummary[] }>('/api/benchmarks/batches', 'list benchmark batches', { signal }),
  benchmarkBatch: (id: string, signal?: AbortSignal) => request<BatchDetail>(batchPath(id), 'load this batch', { signal }),
  benchmarkRunScore: (run: string, signal?: AbortSignal) => request<RunScore>(`/api/benchmarks/runs/${encodeURIComponent(run)}/score`, "load this run's score", { signal }),
  benchmarkCompare: (a: string, b: string, signal?: AbortSignal) => request<BatchCompare>(`/api/benchmarks/compare${queryString({ a, b })}`, 'compare these batches', { signal }),

  // Starting and stopping a batch.
  benchmarkStartOptions: (signal?: AbortSignal) => request<HarnessOptions>('/api/benchmarks/start-options', 'load the start-a-batch form', { signal }),
  benchmarkSlots: (signal?: AbortSignal) => request<RunSlots>('/api/benchmarks/slots', "read the host's run slots", { signal }),
  benchmarkHarnessPreview: (params: { graph: string; runtime: string; model?: string }, signal?: AbortSignal) =>
    request<HarnessPreview>(`/api/benchmarks/harness-preview${queryString({ graph: params.graph, runtime: params.runtime, model: params.model })}`, 'check this harness', { signal }),
  benchmarkStartBatch: (body: StartBatchRequest) => post<{ id: string }>('/api/benchmarks/batches', body, 'start this batch'),
  benchmarkStopBatch: (id: string) => post<{ stopped: boolean; message: string }>(`${batchPath(id)}/stop`, {}, 'stop this batch'),

  // Sweeps.
  benchmarkSweeps: (signal?: AbortSignal) => request<{ sweeps: SweepSummary[] }>('/api/benchmarks/sweeps', 'list sweeps', { signal }),
  benchmarkSweepReport: ({ ids, label, task, budget }: SweepFilter & { budget?: string }, signal?: AbortSignal) =>
    request<SweepReport>(`/api/benchmarks/sweep-report${queryString({ ids: ids.join(','), label, task, budget })}`, 'load the sweep report', { signal }),
  benchmarkWriteRuntime: (body: SweepFilter & { budget?: number; path: string }) => post<SweepReport>('/api/benchmarks/sweep-report/write-runtime', body, 'write the runtime file'),
};

/** Every endpoint, as the pages call them (`api.run(…)`, `api.feature(…)`, `api.benchmarkBatch(…)`). */
export const api = { ...runsApi, ...featuresApi, ...benchmarksApi };

export type { ReplayBody, ReplayResult } from './types';

export function errorMessage(cause: unknown): string {
  if (cause instanceof Error) return cause.message;
  return String(cause);
}

/**
 * Whether a panel's Retry button is worth showing for a failed call: only transport failures
 * (network, 502, 504) may answer differently next time; a 4xx or a 503 (no workspace, no base)
 * answers the same again. Stricter than `useResource`'s own automatic retry (`autoRetryable`).
 */
export function retryable(status: number | undefined): boolean {
  return status == null || status === 0 || status === 502 || status === 504;
}
