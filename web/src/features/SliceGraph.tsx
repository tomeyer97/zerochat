/**
 * The slice graph: a feature's slices as cards in dependency order, laid out by ELK (the same
 * worker as the run graph), its edges coloured by whether the dependency is met. It fits the
 * width when it can, pans when it cannot, and becomes a list on narrow screens.
 */
import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { Flag } from 'lucide-react';
import { ReactFlow, ReactFlowProvider, ViewportPortal, useReactFlow, type Node, type NodeProps } from '@xyflow/react';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { errorMessage } from '../api';
import type { SliceView } from '../types';
import { APPROVAL_SLICE_HINT, EDGE_LABEL, EDGE_STATES, SLICE_CARD, attentionSlice, buildSliceElk, roundedPath, sliceEdges, sliceGraphKey } from '../features-model';
import { ErrorBox, Skeleton } from '../common';
import { CANVAS_PAD, layoutBoth, pickDirection } from '../components/graph-layout';
import type { Direction } from '../graph-model';
import { SliceCard, SliceList, cardLabel, type SliceCardData } from './SliceCard';

/** Never smaller than this: below it the cards are not readable, so the graph pans (or becomes a list). */
const MIN_ZOOM = 0.8;
/** Narrower than this, the slices are a list. */
const LIST_BELOW = 560;
/** The height the direction rule tries to fit the graph in (it grows taller when it must). */
const DIRECTION_FIT_HEIGHT = 720;
/** The canvas's height before the layout is ready. */
const LOADING_HEIGHT = 160;

type GraphProps = { slices: SliceView[]; selected?: string; now: number; daemonUp: boolean; pollSecs?: number; onOpen: (id: string) => void };

export function SliceGraph(props: GraphProps) {
  const wrap = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={wrap} className="slice-graph-host">
      {width > 0 && width < LIST_BELOW ? (
        <SliceList {...props} />
      ) : (
        <ReactFlowProvider>
          <SliceGraphInner {...props} width={width} />
        </ReactFlowProvider>
      )}
      {props.slices.some((s) => s.checkpoint) && (
        <p className="graph-hint muted small slice-legend">
          <Flag size={12} aria-hidden /> {APPROVAL_SLICE_HINT}
        </p>
      )}
    </div>
  );
}

const SliceNode = memo(function SliceNode({ data }: NodeProps<Node<SliceCardData>>) {
  return <SliceCard {...data} />;
});

const nodeTypes = { slice: SliceNode };

/** Lay out, fit, draw: the cards as React Flow nodes, the edges as an SVG on its viewport. */
function SliceGraphInner({ slices, selected, now, daemonUp, pollSecs, onOpen, width }: GraphProps & { width: number }) {
  const { layout, error } = useSliceLayout(slices, width);
  const focus = selected ?? attentionSlice(slices);
  const { zoom, overflows } = useFitViewport(layout, width, focus);
  const canvasHeight = layout ? Math.ceil(layout.height * zoom + 2 * CANVAS_PAD) : LOADING_HEIGHT;
  const statusOf = useMemo(() => {
    const map = new Map(slices.map((s) => [s.id, s.status]));
    return (id: string) => map.get(id);
  }, [slices]);
  const nodes = useMemo(() => {
    if (!layout) return [] as Node<SliceCardData>[];
    return slices.flatMap((s): Node<SliceCardData>[] => {
      const p = layout.placed.get(s.id);
      if (!p) return [];
      return [
        {
          id: s.id,
          type: 'slice',
          position: p,
          width: SLICE_CARD.width,
          height: SLICE_CARD.height,
          style: { ...SLICE_CARD },
          draggable: false,
          connectable: false,
          selectable: false,
          ariaLabel: cardLabel(s, now, daemonUp, statusOf, pollSecs),
          data: { slice: s, selected: s.id === selected, now, daemonUp, pollSecs, statusOf },
        },
      ];
    });
  }, [layout, slices, selected, now, daemonUp, pollSecs, statusOf]);

  if (!slices.length) {
    return (
      <div className="graph-empty">
        <p>The plan has no slices.</p>
      </div>
    );
  }
  return (
    <>
      <div
        className="graph-wrap slice-graph"
        style={{ height: canvasHeight }}
        onKeyDown={(event) => {
          if (event.key !== 'Enter' && event.key !== ' ') return;
          const el = (event.target as HTMLElement).closest('.react-flow__node') as HTMLElement | null;
          if (el?.dataset.id) {
            event.preventDefault();
            onOpen(el.dataset.id);
          }
        }}
      >
        {error && <ErrorBox compact message={error} />}
        {!layout && (
          <div className="graph-loading">
            <Skeleton height={120} />
          </div>
        )}
        <ReactFlow
          nodes={nodes}
          edges={[]}
          nodeTypes={nodeTypes}
          onNodeClick={(_, node) => onOpen(node.id)}
          nodesDraggable={false}
          nodesConnectable={false}
          nodesFocusable
          edgesFocusable={false}
          elementsSelectable={false}
          ariaLabelConfig={{
            'node.a11yDescription.default': 'Press enter or space to open this slice.',
            'node.a11yDescription.keyboardDisabled': 'Press enter or space to open this slice.',
          }}
          minZoom={MIN_ZOOM}
          maxZoom={1}
          proOptions={{ hideAttribution: true }}
          panOnDrag={overflows}
          panOnScroll={false}
          zoomOnScroll={false}
          zoomOnPinch={false}
          zoomOnDoubleClick={false}
          preventScrolling={false}
        >
          {layout && <SliceEdges slices={slices} layout={layout} />}
        </ReactFlow>
      </div>
      {overflows && <p className="graph-hint muted small">Drag the graph to see all {slices.length} slices</p>}
    </>
  );
}

type Point = { x: number; y: number };
type Layout = { placed: Map<string, Point>; edges: Map<string, Point[]>; width: number; height: number };

/** ELK's result as the graph draws it: each card's position and each edge's route. */
function readLayout(result: ElkNode): Layout {
  const placed = new Map<string, Point>();
  for (const c of result.children ?? []) placed.set(c.id, { x: c.x ?? 0, y: c.y ?? 0 });
  const edges = new Map<string, Point[]>();
  for (const e of result.edges ?? []) {
    const section = e.sections?.[0];
    if (section) edges.set(e.id, [section.startPoint, ...(section.bendPoints ?? []), section.endPoint]);
  }
  return { placed, edges, width: result.width ?? 0, height: result.height ?? 0 };
}

/**
 * The slices laid out in both directions (only when ids or dependencies change, not on every
 * status), and the direction that suits `width`. The direction is picked once per structure,
 * so a resize does not flip the graph under the pointer.
 */
function useSliceLayout(slices: SliceView[], width: number): { layout: Layout | undefined; error: string | null } {
  const key = sliceGraphKey(slices);
  const [layouts, setLayouts] = useState<{ key: string; both: Record<Direction, Layout> } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    if (!slices.length) return;
    layoutBoth((direction) => buildSliceElk(slices, direction), readLayout)
      .then((both) => !cancelled && setLayouts({ key, both }))
      .catch((cause) => !cancelled && setError(`The slice graph could not be laid out: ${errorMessage(cause)}`));
    return () => {
      cancelled = true;
    };
    // Only the structure (`key`) changes the layout, not the slices' statuses.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  const ready = layouts?.key === key ? layouts.both : null;
  const picked = useRef<{ key: string; direction: Direction } | null>(null);
  if (ready && width && picked.current?.key !== key) picked.current = { key, direction: pickDirection(ready, width, DIRECTION_FIT_HEIGHT) };
  const direction: Direction = picked.current?.key === key ? picked.current.direction : 'RIGHT';
  return { layout: ready?.[direction], error };
}

/**
 * Shows the whole graph centred when it fits at a readable zoom. When it is too wide even at
 * `MIN_ZOOM`, it starts at `focus` (the open slice, else the first that needs you) and pans.
 */
function useFitViewport(layout: Layout | undefined, width: number, focus: string | undefined): { zoom: number; overflows: boolean } {
  const flow = useReactFlow();
  const fit = layout && width ? Math.min(1, (width - 2 * CANVAS_PAD) / Math.max(1, layout.width)) : 1;
  const zoom = Math.max(MIN_ZOOM, fit);
  const overflows = fit < MIN_ZOOM;
  useEffect(() => {
    if (!layout || !width) return;
    const frame = window.requestAnimationFrame(() => {
      if (!overflows) {
        void flow.setViewport({ x: Math.max(CANVAS_PAD, (width - layout.width * zoom) / 2), y: CANVAS_PAD, zoom });
        return;
      }
      const p = focus ? layout.placed.get(focus) : undefined;
      const x = p ? Math.min(CANVAS_PAD, width / 2 - (p.x + SLICE_CARD.width / 2) * zoom) : CANVAS_PAD;
      void flow.setViewport({ x: Math.max(width - layout.width * zoom - CANVAS_PAD, x), y: CANVAS_PAD, zoom });
    });
    return () => window.cancelAnimationFrame(frame);
    // Not on `focus`: opening another slice does not move the graph.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layout, width, zoom, overflows, flow]);
  return { zoom, overflows };
}

/** The edges, drawn on the viewport as ELK routed them, one arrow head per edge state. */
function SliceEdges({ slices, layout }: { slices: SliceView[]; layout: Layout }) {
  const edges = useMemo(() => sliceEdges(slices), [slices]);
  return (
    <ViewportPortal>
      <svg className="slice-edges" width={layout.width} height={layout.height} aria-hidden focusable="false">
        <defs>
          {EDGE_STATES.map((s) => (
            <marker key={s} id={`slice-arrow-${s}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M0,0 L10,5 L0,10 z" className={`slice-arrow-${s}`} />
            </marker>
          ))}
        </defs>
        {edges.map((e) => {
          const points = layout.edges.get(e.id);
          if (!points) return null;
          return (
            <path key={e.id} d={roundedPath(points)} className={`slice-edge edge-${e.state}`} markerEnd={`url(#slice-arrow-${e.state})`}>
              <title>{`${e.source} → ${e.target}: ${EDGE_LABEL[e.state]}`}</title>
            </path>
          );
        })}
      </svg>
    </ViewportPortal>
  );
}
