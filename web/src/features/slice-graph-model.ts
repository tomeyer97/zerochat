/** The slice graph's model: edge states from the slices' statuses, and the ELK layout input and
 * edge paths the graph draws. */
import type { ElkNode } from 'elkjs/lib/elk-api';
import type { SliceView } from '../types';

export const SLICE_CARD = { width: 212, height: 70 };

/**
 * met: the dependency merged; approval: a checkpoint waits for a person; blocked: the dependency
 * failed or was interrupted; skipped: dropped (the dependent goes without it); open: still to come.
 */
export type EdgeState = 'met' | 'open' | 'approval' | 'blocked' | 'skipped';

/** Each edge state in words (an edge's tooltip). */
export const EDGE_LABEL: Record<EdgeState, string> = {
  met: 'done',
  open: 'to come',
  approval: 'needs approval',
  blocked: 'failed',
  skipped: 'skipped',
};

/** Every edge state (one arrow head each). */
export const EDGE_STATES = Object.keys(EDGE_LABEL) as EdgeState[];
export type SliceEdge = { id: string; source: string; target: string; state: EdgeState };

export function sliceEdges(slices: Pick<SliceView, 'id' | 'deps' | 'status'>[]): SliceEdge[] {
  const ids = new Set(slices.map((s) => s.id));
  const status = new Map(slices.map((s) => [s.id, s.status]));
  const state = (d: string): EdgeState => {
    switch (status.get(d)) {
      case 'merged':
        return 'met';
      case 'skipped':
        return 'skipped';
      case 'waiting_for_review':
      case 'waiting_for_approval':
        return 'approval';
      case 'failed':
      case 'interrupted':
        return 'blocked';
      default:
        return 'open';
    }
  };
  return slices.flatMap((s) => s.deps.filter((d) => ids.has(d)).map((d) => ({ id: `${d}->${s.id}`, source: d, target: s.id, state: state(d) })));
}

/** Structure only (ids and deps): the layout is redone only when this changes. */
export function sliceGraphKey(slices: Pick<SliceView, 'id' | 'deps'>[]): string {
  return slices.map((s) => `${s.id}<${s.deps.join(',')}`).join(';');
}

/**
 * Columns by dependency depth (slices without dependencies in the first), and orthogonal edge
 * routes from ELK, each with its own end point, so no edge runs under a card or shares an arrow.
 */
export function buildSliceElk(slices: Pick<SliceView, 'id' | 'deps' | 'status'>[], direction: 'RIGHT' | 'DOWN' = 'RIGHT'): ElkNode {
  return {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': direction,
      'elk.edgeRouting': 'ORTHOGONAL',
      'elk.layered.layering.strategy': 'LONGEST_PATH_SOURCE',
      'elk.spacing.nodeNode': direction === 'RIGHT' ? '22' : '26',
      'elk.layered.spacing.nodeNodeBetweenLayers': direction === 'RIGHT' ? '46' : '36',
      'elk.layered.spacing.edgeNodeBetweenLayers': '14',
      'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.padding': '[top=8,left=8,bottom=8,right=8]',
    },
    children: slices.map((s) => ({ id: s.id, ...SLICE_CARD })),
    edges: sliceEdges(slices).map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
  };
}

/** An orthogonal polyline with softly rounded corners, as an SVG path. */
export function roundedPath(points: { x: number; y: number }[], radius = 6): string {
  if (points.length < 2) return '';
  let d = `M ${points[0].x} ${points[0].y}`;
  for (let i = 1; i < points.length - 1; i++) {
    const [a, b, c] = [points[i - 1], points[i], points[i + 1]];
    const r = Math.min(radius, Math.hypot(b.x - a.x, b.y - a.y) / 2, Math.hypot(c.x - b.x, c.y - b.y) / 2);
    const p1 = { x: b.x - Math.sign(b.x - a.x) * r, y: b.y - Math.sign(b.y - a.y) * r };
    const p2 = { x: b.x + Math.sign(c.x - b.x) * r, y: b.y + Math.sign(c.y - b.y) * r };
    d += ` L ${p1.x} ${p1.y} Q ${b.x} ${b.y} ${p2.x} ${p2.y}`;
  }
  const last = points[points.length - 1];
  return `${d} L ${last.x} ${last.y}`;
}
