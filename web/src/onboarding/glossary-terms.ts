// The glossary behind the "?" in the top bar: one short line per term, for the words the UI uses
// for Zeroshot runs and zsf features.

export type GlossaryTerm = { term: string; meaning: string };

export const GLOSSARY: GlossaryTerm[] = [
  { term: 'Run', meaning: 'One `zeroshot run`: a graph of agents working on one task.' },
  { term: 'Node', meaning: 'One step of the graph, usually an agent (worker, reviewer…).' },
  { term: 'Round', meaning: 'One pass through the graph’s loop, e.g. work, then review.' },
  { term: 'Execution', meaning: 'One time a node ran; a node runs again in each round.' },
  { term: 'Steer', meaning: 'A message to a running run, or to one agent, that it reads while it works.' },
  { term: 'Checkpoint', meaning: 'A snapshot of the run’s workspace; you can start again from it.' },
  { term: 'Background agents', meaning: 'Helper agents (Active Memory) beside the nodes; Zeroshot does not price them.' },
  { term: 'Feature', meaning: 'A plan of slices that zsf (Zeroshot features) runs for you.' },
  { term: 'Slice', meaning: 'One part of a feature, done by one Zeroshot run.' },
  { term: 'Attempt', meaning: 'One try at a slice; a retry is a new attempt.' },
  { term: 'Approval slice', meaning: 'A slice that waits for your review before it merges.' },
  { term: 'Gate', meaning: 'The checks (build and tests) a slice passes before it merges.' },
];
