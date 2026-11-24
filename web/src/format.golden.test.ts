// Goldens for format.ts over a table of edge values and every run the API snapshots hold.
// golden.ts pins TZ=UTC, Date.now() and the en-US locale first: several of these format local time.
import { FIXED_NOW, apiBodies, checkGolden } from './testdata/golden';
import { test } from 'node:test';
import {
  clockTime,
  compact,
  costText,
  duration,
  fullTime,
  offsetText,
  plural,
  prettyJson,
  relativeAge,
  shortId,
  shortModel,
  timeOfDay,
  tokenParts,
  totalTokens,
  truncate,
  untilText,
  usageBreakdown,
  usd,
} from './format';
import type { RunView, Usage } from './types';

const runs = apiBodies<RunView>('runs', /(^|\/)run\.json$/).map(([name, view]) => [name.replace(/\/run$/, ''), view] as [string, RunView]);
const executions = runs.flatMap(([name, view]) => view.executions.map((e) => [`${name}#${e.id}`, e, view] as const));

const NUMBERS = [
  null,
  undefined,
  NaN,
  Infinity,
  -Infinity,
  -1500,
  -0.4,
  0,
  0.004,
  0.005,
  0.01,
  0.499,
  0.5,
  1,
  9.95,
  42,
  999,
  999.5,
  1000,
  1049,
  1050,
  9_949,
  9_950,
  59_999,
  60_000,
  61_000,
  999_499,
  999_500,
  1_000_000,
  3_599_999,
  3_600_000,
  9_999_999,
  86_399_999,
  172_799_999,
  172_800_000,
  1e9,
  1.5e9,
  1e12,
];
const AT = [
  null,
  undefined,
  0,
  FIXED_NOW,
  FIXED_NOW - 1,
  FIXED_NOW - 44_999,
  FIXED_NOW - 45_000,
  FIXED_NOW - 89_999,
  FIXED_NOW - 3_570_000,
  FIXED_NOW - 86_400_000,
  FIXED_NOW - 13 * 86_400_000,
  FIXED_NOW - 14 * 86_400_000,
  FIXED_NOW - 400 * 86_400_000,
  FIXED_NOW + 3_600_000,
  Date.UTC(2026, 0, 1),
  Date.UTC(2025, 11, 31, 23, 59, 59),
  Date.UTC(2026, 10, 20, 0, 0, 0),
  Date.UTC(2026, 10, 20, 23, 59, 59),
  Date.UTC(2026, 10, 19, 12, 0, 0),
];
const USAGES: (Usage | null | undefined)[] = [
  null,
  undefined,
  { inputTokens: 0, outputTokens: 0, cacheReadInputTokens: 0, cacheCreationInputTokens: 0 },
  { inputTokens: 10, outputTokens: 5, cacheReadInputTokens: 15, cacheCreationInputTokens: 0 },
  { inputTokens: 0, outputTokens: 0, cacheReadInputTokens: 1000, cacheCreationInputTokens: 0 },
  { inputTokens: 1234, outputTokens: 5678, cacheReadInputTokens: 330_000, cacheCreationInputTokens: 2_100 },
  { inputTokens: NaN, outputTokens: undefined as unknown as number, cacheReadInputTokens: 5, cacheCreationInputTokens: 0 },
];
const label = (v: unknown) => (v === undefined ? 'undefined' : typeof v === 'number' && !Number.isFinite(v) ? String(v) : JSON.stringify(v));
const table = <T>(values: T[], f: (v: T) => unknown) => values.map((v) => [label(v), f(v)]);

test('golden: duration', () => {
  checkGolden('format/duration', {
    table: table(NUMBERS, duration),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, duration(v.endedAt != null && v.startedAt != null ? v.endedAt - v.startedAt : null)])),
    executions: Object.fromEntries(executions.map(([n, e]) => [n, duration(e.durationMs)])),
  });
});

test('golden: relativeAge', () => {
  checkGolden('format/relativeAge', {
    table: table(AT, (at) => relativeAge(at, FIXED_NOW)),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, relativeAge(v.createdAt, FIXED_NOW)])),
  });
});

test('golden: compact', () => {
  checkGolden('format/compact', {
    table: table(NUMBERS, compact),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, compact(totalTokens(v.usage))])),
  });
});

test('golden: usd', () => {
  checkGolden('format/usd', {
    table: table(NUMBERS, usd),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, usd(v.costUsd)])),
    executions: Object.fromEntries(executions.map(([n, e]) => [n, usd(e.costUsd)])),
  });
});

test('golden: totalTokens, usageBreakdown, tokenParts', () => {
  for (const [name, f] of [
    ['totalTokens', totalTokens],
    ['usageBreakdown', usageBreakdown],
    ['tokenParts', tokenParts],
  ] as const) {
    checkGolden(`format/${name}`, {
      table: USAGES.map((u) => [label(u), (f as (u: Usage | null | undefined) => unknown)(u)]),
      runs: Object.fromEntries(runs.map(([n, v]) => [n, { usage: f(v.usage), nodeUsage: f(v.nodeUsage), backgroundUsage: f(v.backgroundUsage) }])),
      executions: Object.fromEntries(executions.map(([n, e]) => [n, f(e.usage)])),
    });
  }
});

test('golden: clockTime, fullTime, timeOfDay', () => {
  checkGolden('format/clockTime', {
    table: table(AT, (at) => [clockTime(at), clockTime(at, true)]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, [clockTime(v.startedAt), clockTime(v.startedAt, true)]])),
  });
  checkGolden('format/fullTime', {
    table: table(AT, (at) => [fullTime(at, FIXED_NOW), fullTime(at), fullTime(at, Date.UTC(2030, 0, 1))]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, fullTime(v.createdAt, FIXED_NOW)])),
  });
  checkGolden('format/timeOfDay', {
    table: table(AT, (at) => [timeOfDay(at, FIXED_NOW), timeOfDay(at), timeOfDay(at, Date.UTC(2030, 0, 1))]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, [timeOfDay(v.createdAt, FIXED_NOW), timeOfDay(v.startedAt, v.startedAt ?? FIXED_NOW)]])),
  });
});

test('golden: offsetText', () => {
  checkGolden('format/offsetText', {
    table: [
      [null, 0],
      [0, null],
      [0, 0],
      [999, 0],
      [1000, 0],
      [59_999, 0],
      [60_000, 0],
      [3_600_000, 0],
      [0, 5000],
    ].map(([at, start]) => [`${at} from ${start}`, offsetText(at, start)]),
    executions: Object.fromEntries(executions.map(([n, e, v]) => [n, offsetText(e.startedAt, v.startedAt)])),
  });
});

test('golden: shortId, shortModel', () => {
  const ids = [null, undefined, '', 'abc', '12345678', '123456789', '0190c3a2-7b1e-7c00-8000-00000000abcd'];
  checkGolden('format/shortId', {
    table: table(ids, (id) => [shortId(id), shortId(id, 4), shortId(id, 0)]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, shortId(v.runId)])),
  });
  const models = [null, undefined, '', 'claude-haiku-4-5-20251001', 'claude-sonnet-5', 'gpt-5-codex', 'claude-opus-4-1-2025', 'claude-', 'x-claude-y-12345678'];
  checkGolden('format/shortModel', {
    table: table(models, shortModel),
    executions: Object.fromEntries(executions.map(([n, e]) => [n, shortModel(e.model)])),
  });
});

test('golden: plural, truncate, untilText', () => {
  checkGolden('format/plural', {
    table: [0, 1, 2, -1, 1.5].flatMap((n) => [
      [`${n} file`, plural(n, 'file')],
      [`${n} batch/batches`, plural(n, 'batch', 'batches')],
    ]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, plural(v.executions.length, 'execution')])),
  });
  const texts = ['', 'a', 'abcdef', 'abcdefghij', 'é😀ü'];
  checkGolden('format/truncate', {
    table: texts.flatMap((t) => [-1, 0, 1, 2, 5, 6, 10].map((max) => [`${JSON.stringify(t)} ${max}`, truncate(t, max)])),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, truncate(v.title, 24)])),
  });
  checkGolden('format/untilText', {
    table: [-1, 0, 30_000, 30_001, 60_000, 479_999, 480_000, 3_599_999, 3_600_000, 3_660_000, 7_500_000, 200_000_000].map((ms) => [String(ms), untilText(FIXED_NOW + ms, FIXED_NOW)]),
    runs: Object.fromEntries(runs.filter(([, v]) => v.failure?.resetsAt != null).map(([n, v]) => [n, untilText(v.failure!.resetsAt!, FIXED_NOW)])),
  });
});

test('golden: prettyJson', () => {
  const cyclic: Record<string, unknown> = { a: 1 };
  cyclic.self = cyclic;
  checkGolden('format/prettyJson', {
    table: [
      ['undefined', prettyJson(undefined)],
      ['null', prettyJson(null)],
      ['function', prettyJson(() => 1)],
      ['symbol', prettyJson(Symbol('s'))],
      ['bigint', prettyJson(BigInt(5))],
      ['cyclic', prettyJson(cyclic)],
      ['nested', prettyJson({ b: [1, { c: 'x' }], a: null })],
      ['string', prettyJson('text\nline')],
      ['NaN', prettyJson(NaN)],
    ],
    runs: Object.fromEntries(runs.map(([n, v]) => [n, prettyJson(v.initialInput)])),
  });
});

test('golden: costText', () => {
  checkGolden('format/costText', {
    table: table(NUMBERS, (v) => [costText(v, false), costText(v, true)]),
    runs: Object.fromEntries(runs.map(([n, v]) => [n, costText(v.costUsd, v.costEstimated)])),
    executions: Object.fromEntries(executions.map(([n, e]) => [n, costText(e.costUsd, e.costSource === 'estimate')])),
  });
});
