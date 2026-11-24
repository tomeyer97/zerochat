// The Benchmarks pages, one module each under benchmarks/: the batches list, a batch's page, the
// run score card, the Start form, the sweeps list and report, and the compare page. This file
// re-exports them, so App.tsx and RunPage.tsx keep importing from one place. Each page sits
// behind PbGate: without pb (the ProgramBench runner) it is one neutral page instead.
import { BatchList } from './benchmarks/BatchList';
import { BatchPage } from './benchmarks/BatchPage';
import { Compare } from './benchmarks/Compare';
import { StartForm } from './benchmarks/StartForm';
import { SweepReport } from './benchmarks/SweepReport';
import { Sweeps } from './benchmarks/Sweeps';
import { gated } from './benchmarks/PbGate';

export const BenchmarksList = gated(BatchList);
export const BenchmarkBatchPage = gated(BatchPage);
export const BenchmarksStart = gated(StartForm);
export const BenchmarksSweeps = gated(Sweeps);
export const BenchmarksSweepReport = gated(SweepReport);
export const BenchmarksCompare = gated(Compare);
export { ScoreCard } from './benchmarks/ScoreCard';
export { HarnessNodes } from './benchmarks/WhatRan';
export { harnessName } from './benchmarks/harness-text';
