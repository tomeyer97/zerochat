import { memo, useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import {
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  Handle,
  Position,
  ReactFlow,
  ReactFlowProvider,
  getSmoothStepPath,
  useReactFlow,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
} from '@xyflow/react';
import { Columns3, Flag, GitFork, OctagonX, Repeat2, Split, type LucideIcon } from 'lucide-react';
import type { Pressure, RunView } from './types';
import { buildElkGraph, buildFlowModel, commonModel, humanId, readElkLayout, showEveryModel, structureKey, type Direction, type FlowModel, type GroupKind } from './graph-model';
import {
  agentCard,
  autoViewport,
  cx,
  exitTitle,
  focusNode,
  graphView,
  groupHeader,
  maxCanvasHeight,
  toFlowElements,
  type CardData,
  type EdgeData,
  type GraphView,
  type Layout,
} from './graph-view-model';
import { isLive, runStatus } from './status';
import { StateIcon, ErrorBox, Skeleton } from './common';
import { useNow } from './hooks';
import { fitZoom, layoutBoth, pickDirection } from './components/graph-layout';

// ---------- nodes and edges ----------

function handles(direction: Direction) {
  return direction === 'RIGHT' ? { target: Position.Left, source: Position.Right } : { target: Position.Top, source: Position.Bottom };
}

const AgentNode = memo(function AgentNode({ data }: NodeProps<Node<CardData>>) {
  const { item } = data;
  const now = useNow(item.state === 'running');
  const card = agentCard(data, now);
  const h = handles(data.direction);
  return (
    <div className={card.className} title={card.title}>
      <Handle type="target" position={h.target} />
      <div className="card-head">
        <StateIcon state={item.state} size={14} />
        <span className="card-name">{item.id}</span>
      </div>
      {card.meta.length > 0 && (
        <div className="card-meta">
          {card.meta.map((part) => (
            <span key={part.key} className={part.className}>
              {part.text}
            </span>
          ))}
        </div>
      )}
      <Handle type="source" position={h.source} />
    </div>
  );
});

/** A choice nothing leads into (rare): it cannot be folded into an edge. */
const DecisionNode = memo(function DecisionNode({ data }: NodeProps<Node<CardData>>) {
  const h = handles(data.direction);
  return (
    <div className={`flow-card decision ${data.item.visited ? 'visited' : 'unvisited'}`} title={`Choice ${data.item.id}: takes the first branch whose condition holds`}>
      <Handle type="target" position={h.target} />
      <Split size={12} aria-hidden />
      <span>{humanId(data.item.id)}</span>
      <Handle type="source" position={h.source} />
    </div>
  );
});

/** The exit this run reached (the others are folded into their source's tooltip). */
const ExitNode = memo(function ExitNode({ data }: NodeProps<Node<CardData>>) {
  const { item } = data;
  const h = handles(data.direction);
  const fail = item.kind === 'fail';
  return (
    <div className={`flow-card exit ${fail ? 'exit-fail' : 'exit-succeed'}`} title={exitTitle(item)}>
      <Handle type="target" position={h.target} />
      {fail ? <OctagonX size={12} aria-hidden /> : <Flag size={12} aria-hidden />}
      <span>{humanId(item.id)}</span>
    </div>
  );
});

const GROUP_ICONS: Record<GroupKind, LucideIcon> = { loop: Repeat2, par: Columns3, map: GitFork };

const GroupNode = memo(function GroupNode({ data }: NodeProps<Node<CardData>>) {
  const { item } = data;
  const h = handles(data.direction);
  const Icon = item.groupKind ? GROUP_ICONS[item.groupKind] : GitFork;
  const header = groupHeader(item, data.live);
  return (
    <div className={cx('flow-group', `group-${item.groupKind}`, !item.visited && 'unvisited')}>
      <Handle type="target" position={h.target} />
      <div className="group-header" title={header.title}>
        <Icon size={12} aria-hidden />
        <span>{header.caption}</span>
        {header.round && <span className={cx('group-round tabular', header.round.pressing && 'level-warning')}>{header.round.text}</span>}
      </div>
      <Handle type="source" position={h.source} />
    </div>
  );
});

function LabeledEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, data, style }: EdgeProps<Edge<EdgeData>>) {
  const [path, labelX, labelY] = getSmoothStepPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, borderRadius: 10, offset: 14 });
  // "otherwise" says no other condition matched, not why this branch runs: only worth printing
  // on the canvas when it names an actual condition. A loop-back edge is always worth labelling:
  // it is the one edge a reader would not expect.
  const showLabel = !!data?.label && data.label !== 'otherwise' && (data.branch || data.loopBack) && data.taken;
  return (
    <>
      <BaseEdge id={id} path={path} markerEnd={markerEnd} style={style} interactionWidth={0} />
      {data?.title && (
        <path d={path} className="edge-hit" fill="none" strokeOpacity={0} strokeWidth={12}>
          <title>{data.title}</title>
        </path>
      )}
      {showLabel && (
        <EdgeLabelRenderer>
          <div className="edge-label" style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }} title={data.title}>
            {data.label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}

const nodeTypes = { agent: AgentNode, decision: DecisionNode, exit: ExitNode, group: GroupNode };
const edgeTypes = { labeled: LabeledEdge };

// ---------- hooks ----------

/** Both orientations of the model laid out by ELK, once per structure (state changes re-render in place). */
function useElkLayouts(model: FlowModel, key: string): { layouts: Record<Direction, Layout> | null; error: string | null } {
  const [layouts, setLayouts] = useState<({ key: string } & Record<Direction, Layout>) | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    setError(null);
    if (!model.items.length) return;
    layoutBoth(
      (direction) => buildElkGraph(model, direction),
      (result): Layout => ({ placed: readElkLayout(result), width: result.width ?? 0, height: result.height ?? 0 })
    )
      .then((both) => {
        if (!cancelled) setLayouts({ key, ...both });
      })
      .catch((cause) => {
        if (!cancelled) setError(`The graph could not be laid out: ${cause instanceof Error ? cause.message : String(cause)}`);
      });
    return () => {
      cancelled = true;
    };
    // Only the structure changes the layout; state updates re-render in place.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return { layouts: layouts?.key === key ? layouts : null, error };
}

/**
 * The element's width, kept current. `resized` turns true on every resize after the first
 * measurement, for the viewport to notice (and reset).
 */
function useElementWidth(ref: RefObject<HTMLElement | null>, mounted: boolean) {
  const [width, setWidth] = useState(0);
  const resized = useRef(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.clientWidth);
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      resized.current = true;
      setWidth(el.clientWidth);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, mounted]);
  return { width, resized };
}

/**
 * The orientation, decided once per layout (from the canvas width when it first becomes
 * available) and then held: opening a side panel, or dragging its divider, narrows this same
 * canvas and must not flip left-to-right into top-to-bottom under the person.
 */
function useHeldDirection(key: string, layouts: Record<Direction, Layout> | null, width: number, maxHeight: number): Direction {
  const held = useRef<{ key: string; direction: Direction } | null>(null);
  if (layouts && width && held.current?.key !== key) {
    held.current = { key, direction: pickDirection(layouts, width, maxHeight) };
  }
  return held.current?.key === key ? held.current.direction : 'RIGHT';
}

/**
 * Shows the whole graph at the size that fits (or, when that is unreadably small, the focus at
 * a readable zoom) until the person pans or zooms. A new structure or a resized canvas (the
 * panel opening, its divider dragged) is not the person choosing to keep their view: both refit.
 * `hasPanned` says whether to offer a way back to the fitted view.
 */
function useAutoViewport(opts: { key: string; layout: Layout | undefined; width: number; resized: RefObject<boolean>; view: GraphView; focusId: string | undefined }) {
  const { key, layout, width, resized, view, focusId } = opts;
  const flow = useReactFlow();
  // The ref decides whether to refit, so the refit effect does not depend on it; the state only
  // shows the way back (a stray drag can pan the graph off-screen even at zoom 1).
  const userMoved = useRef(false);
  const [hasPanned, setHasPanned] = useState(false);
  const forget = () => {
    userMoved.current = false;
    setHasPanned(false);
  };
  useEffect(forget, [key]);
  const { zoom, readable, canvasHeight } = view;
  useEffect(() => {
    if (resized.current) {
      resized.current = false;
      forget();
    }
    if (!layout || !width || userMoved.current) return;
    const frame = window.requestAnimationFrame(() => {
      void flow.setViewport(autoViewport(layout, width, { zoom, readable, canvasHeight }, focusId));
    });
    return () => window.cancelAnimationFrame(frame);
  }, [layout, width, zoom, readable, canvasHeight, focusId, flow]);
  const onMoveStart = (event: MouseEvent | TouchEvent | null) => {
    if (!event) return; // moved by code, not by the person
    userMoved.current = true;
    setHasPanned(true);
  };
  return { hasPanned, onMoveStart };
}

// ---------- the graph ----------

export function Graph(props: { view: RunView; selectedNode?: string; onOpen: (node: string) => void }) {
  return (
    <ReactFlowProvider>
      <GraphInner {...props} />
    </ReactFlowProvider>
  );
}

/** Copy for React Flow's keyboard help: this graph is read-only, so nothing can be moved or deleted. */
const ARIA_LABELS = {
  'node.a11yDescription.default': "Press enter or space to open this node's chat.",
  'node.a11yDescription.keyboardDisabled': "Press enter or space to open this node's chat.",
  'edge.a11yDescription.default': '',
};

function GraphInner({ view, selectedNode, onOpen }: { view: RunView; selectedNode?: string; onOpen: (node: string) => void }) {
  const model = useMemo(() => buildFlowModel(view), [view]);
  const key = structureKey(model.items);
  const hasItems = model.items.length > 0;
  const { layouts, error } = useElkLayouts(model, key);
  const wrap = useRef<HTMLDivElement>(null);
  const { width, resized } = useElementWidth(wrap, hasItems);
  const maxHeight = maxCanvasHeight();
  const direction = useHeldDirection(key, layouts, width, maxHeight);
  const layout = layouts?.[direction];
  const fitted = graphView(layout, layout && width ? fitZoom(layout, width, maxHeight) : 1, maxHeight);
  const focusId = focusNode(model, selectedNode, view.failure?.node);
  const { hasPanned, onMoveStart } = useAutoViewport({ key, layout, width, resized, view: fitted, focusId });

  const pressuresByNode = useMemo(() => {
    const map = new Map<string, Pressure[]>();
    for (const p of view.pressures) if (p.node) map.set(p.node, [...(map.get(p.node) ?? []), p]);
    return map;
  }, [view.pressures]);
  const usual = useMemo(() => commonModel(model.items), [model.items]);
  const everyModel = useMemo(() => showEveryModel(model.items, view.context?.kind === 'benchmark'), [model.items, view.context?.kind]);
  const live = isLive(runStatus(view));
  const { nodes, edges } = useMemo(
    () =>
      toFlowElements(model, layout, {
        direction,
        selectedNode,
        pressuresByNode,
        costEstimated: view.costEstimated,
        model: usual,
        everyModel,
        failureExecution: view.failure?.execution,
        live,
      }),
    [model, layout, direction, selectedNode, pressuresByNode, view.costEstimated, usual, everyModel, view.failure?.execution, live]
  );

  if (!hasItems) {
    return (
      <div className="graph-empty">
        <p>No graph{view.graph ? ' (its root has no nodes)' : ' (the target did not send the graph definition)'}.</p>
      </div>
    );
  }
  return (
    <div
      className="graph-wrap"
      ref={wrap}
      style={{ height: fitted.canvasHeight }}
      onKeyDown={(event) => {
        // Agent nodes are focusable; Enter or Space opens the focused node's chat.
        if (event.key !== 'Enter' && event.key !== ' ') return;
        const el = (event.target as HTMLElement).closest('.react-flow__node-agent') as HTMLElement | null;
        if (el?.dataset.id) {
          event.preventDefault();
          onOpen(el.dataset.id);
        }
      }}
    >
      {error && <ErrorBox compact message={error} />}
      {!layout ? (
        <div className="graph-loading">
          <Skeleton height={120} />
        </div>
      ) : null}
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodeClick={(_, node) => {
          if (node.type === 'agent') onOpen(node.id);
        }}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        ariaLabelConfig={ARIA_LABELS}
        minZoom={0.15}
        maxZoom={1.75}
        proOptions={{ hideAttribution: true }}
        onMoveStart={onMoveStart}
        panOnScroll={false}
        zoomOnScroll={false}
        preventScrolling={false}
      >
        {/* At full size the whole graph is already in view, until a stray drag pans it away;
            then show the same way back as a zoomed-out graph gets. */}
        {(fitted.zoom < 1 || hasPanned) && <Controls showInteractive={false} position="bottom-right" fitViewOptions={{ padding: 0.04, maxZoom: 1 }} />}
      </ReactFlow>
    </div>
  );
}
