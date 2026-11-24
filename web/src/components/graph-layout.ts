/**
 * Graph layout shared by the run graph (Graph.tsx) and the slice graph (Features.tsx): one ELK
 * worker for the whole app, both orientations laid out at once, and the rule that picks one.
 */
import ELK, { type ElkNode } from 'elkjs/lib/elk-api';
import workerUrl from 'elkjs/lib/elk-worker.min.js?url';
import type { Direction } from '../graph-model';

const elk = new ELK({ workerUrl });

/** Margin (px) kept between a laid-out graph and the edge of its canvas. */
export const CANVAS_PAD = 8;

type Size = { width: number; height: number };

/** Lays the graph out left-to-right and top-down; `read` turns each ELK result into the caller's layout. */
export async function layoutBoth<L>(build: (direction: Direction) => ElkNode, read: (result: ElkNode) => L): Promise<Record<Direction, L>> {
  const run = (direction: Direction) => elk.layout(build(direction)).then(read);
  const [RIGHT, DOWN] = await Promise.all([run('RIGHT'), run('DOWN')]);
  return { RIGHT, DOWN };
}

/** Zoom that shows the whole layout in the canvas (never above 1). */
export function fitZoom(layout: Size, width: number, maxHeight: number) {
  return Math.min(1, (width - 2 * CANVAS_PAD) / Math.max(1, layout.width), (maxHeight - 2 * CANVAS_PAD) / Math.max(1, layout.height));
}

/** Left-to-right unless top-down shows the graph clearly larger in this canvas. */
export function pickDirection(layouts: Record<Direction, Size>, width: number, maxHeight: number): Direction {
  return fitZoom(layouts.DOWN, width, maxHeight) > fitZoom(layouts.RIGHT, width, maxHeight) * 1.1 ? 'DOWN' : 'RIGHT';
}
