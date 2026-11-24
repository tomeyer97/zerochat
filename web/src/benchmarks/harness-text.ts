// What was benchmarked, in words: the harness's name, its agent nodes and their models.
import type { BatchDetail, HarnessInfo, HarnessNode } from '../types';

/** A model name without its `claude-` prefix (`sonnet-5`). Unlike format.ts `shortModel`, a date
 * suffix stays: these lines show the model exactly as pb's runtime names it. */
export function bareModel(model: string): string {
  return model.replace(/^claude-/, '');
}

/** Consecutive nodes in the same loop, so the batch page can draw one bracket per loop
 * ("review_loop, up to 3 rounds: coverage ‖ bugfinder → triage → repair"). */
export type HarnessSegment = { loopName: string | null; loopMax: number | null; nodes: HarnessNode[] };

export function harnessSegments(nodes: HarnessNode[]): HarnessSegment[] {
  const out: HarnessSegment[] = [];
  for (const n of nodes) {
    const last = out[out.length - 1];
    if (last && last.loopName === n.loopName) last.nodes.push(n);
    else out.push({ loopName: n.loopName, loopMax: n.loopMax, nodes: [n] });
  }
  return out;
}

/** `sonnet-5 · high`, the model without its `claude-` prefix, plus the effort when set. */
export function nodeModelText(n: Pick<HarnessNode, 'model' | 'effort'>): string {
  const model = n.model ? bareModel(n.model) : 'no model';
  return n.effort ? `${model} · ${n.effort}` : model;
}

/** A node chip's tooltip: `build: writes code, claude-sonnet-5, high effort, in parallel group p`. */
export function nodeTitle(n: HarnessNode): string {
  const role = n.kind === 'verifier' ? 'reviews the work' : 'writes code';
  const model = n.model ? `, ${n.model}` : ', no model in the runtime';
  const effort = n.effort ? `, ${n.effort} effort` : '';
  const parallel = n.parallel ? `, in parallel group ${n.parallel}` : '';
  return `${n.name}: ${role}${model}${effort}${parallel}`;
}

/** The separator drawn before the node at `i`: `‖` inside one parallel group, `→` otherwise. */
export function nodeSeparator(nodes: HarnessNode[], i: number): string | null {
  if (i === 0) return null;
  const prev = nodes[i - 1];
  return prev.parallel && prev.parallel === nodes[i].parallel ? ' ‖ ' : ' → ';
}

/** "variant clone-v1", "default harness (pb.toml)", "custom graph + runtime (…)", plus the
 * runtime when it is not the directory's own `runtime.json`. */
export function harnessName(h: HarnessInfo): string {
  return `${harnessBase(h)}${runtimeSuffix(h)}`;
}

function harnessBase(h: HarnessInfo): string {
  if (h.source === 'variant') return `variant ${h.variant}`;
  if (h.source === 'default') return 'default harness (pb.toml)';
  return 'custom graph + runtime (matches no file under harness/ now)';
}

function runtimeSuffix(h: HarnessInfo): string {
  if (h.runtimeFile) return ` with ${h.runtimeFile}`;
  if (h.source !== 'custom' && !h.runtimeMatches) return ' with a changed runtime';
  return '';
}

/** `harness version 3 · zeroshot 16.1.0`, the batch's versions (the harness line's tooltip). */
export function versionTitle(detail: Pick<BatchDetail, 'harnessVersion' | 'zeroshotVersion'>): string {
  const parts = [detail.harnessVersion ? `harness version ${detail.harnessVersion}` : null, detail.zeroshotVersion ? `zeroshot ${detail.zeroshotVersion}` : null];
  return parts.filter(Boolean).join(' · ');
}
