/**
 * Run and execution status, as the pages show it. The rules live in one module per subject;
 * this root re-exports them so `import … from './status'` keeps working:
 *   run-status.ts     a run's status, its label and tone, live or settled
 *   run-list.ts       the run list: order, status groups, search, subtitle, polling cadence
 *   exec-state.ts     one execution: state, mark, verdict, labels, which visit a link names, j/k
 *   failure-class.ts  why a run failed
 *   run-limits.ts     what a source cannot show or do (missing data, observe-only sources)
 */
export * from './run-status';
export * from './run-list';
export * from './exec-state';
export * from './failure-class';
export * from './run-limits';
