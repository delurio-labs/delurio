import ElkModule from 'elkjs/lib/elk.bundled.js';
import type { ELK as ElkApi, ElkExtendedEdge, ElkNode } from 'elkjs/lib/elk.bundled.js';
import { collectEdges, kindOf, nodeLine, wrap } from './edges.ts';
import type { VizEdge } from './edges.ts';
import { titleOf } from './graph.ts';
import type { Graph } from './graph.ts';
import type { Exploration } from './model.ts';
import type { PathStep } from './story.ts';

// elkjs ships CommonJS: under Node ESM the default import is module.exports, which is the constructor
const Elk = ElkModule as unknown as new () => ElkApi;

export type View = 'main' | 'full';

const START = '__start';
const CHAR_TITLE = 8.4; // 14px semibold sans
const CHAR_LINE = 6.9; // 11.5px monospace
const CHAR_LABEL = 7.1; // 13px sans
const LABEL_WRAP = { main: 26, full: 22 };

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const r1 = (n: number): number => Math.round(n * 10) / 10;

function labelLines(e: VizEdge, view: View): string[] {
  return e.labels.flatMap((l) => wrap(l, LABEL_WRAP[view]));
}

export async function renderSpine(g: Graph, ex: Exploration, view: View, story: PathStep[] | null): Promise<string> {
  const all = collectEdges(g, ex);
  // main view: the story without failure branches and without "the user closes the page"
  const edges = view === 'full' ? all : all.filter((e) => e.kind !== 'fail' && !e.leave);
  const visible = new Set<string>([g.flow.initial]);
  for (const e of edges) {
    visible.add(e.from);
    visible.add(e.to);
  }
  const names = Object.keys(g.flow.states).filter((n) => visible.has(n));

  const mainEdges = new Set<string>();
  const mainNodes = new Set<string>();
  (story ?? []).forEach((s, k) => {
    mainNodes.add(s.state);
    if (k > 0) mainEdges.add(`${story?.[k - 1].state}>${s.state}`);
  });

  const children: ElkNode[] = [{ id: START, width: 14, height: 14 }];
  for (const name of names) {
    const width = Math.ceil(Math.max(150, titleOf(g.flow, name).length * CHAR_TITLE, nodeLine(g, name).length * CHAR_LINE) + 36);
    const node: ElkNode = { id: name, width, height: 58 };
    if (kindOf(g, name) === 'exit') node.layoutOptions = { 'elk.layered.layering.layerConstraint': 'LAST' };
    children.push(node);
  }
  const elkEdges: ElkExtendedEdge[] = [{ id: 'e_start', sources: [START], targets: [g.flow.initial] }];
  edges.forEach((e, i) => {
    const lines = labelLines(e, view);
    elkEdges.push({
      id: `e${i}`,
      sources: [e.from],
      targets: [e.to],
      labels:
        lines.length === 0
          ? []
          : [{ text: lines.join('\n'), width: Math.ceil(Math.max(...lines.map((l) => l.length)) * CHAR_LABEL + 14), height: lines.length * 17 + 6 }],
    });
  });
  const root: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'DOWN',
      'elk.edgeRouting': 'ORTHOGONAL',
      'elk.padding': '[top=24,left=24,bottom=24,right=24]',
      'elk.spacing.nodeNode': '36',
      'elk.spacing.edgeNode': '22',
      'elk.spacing.edgeEdge': '16',
      'elk.spacing.edgeLabel': '6',
      'elk.layered.spacing.nodeNodeBetweenLayers': '56',
      'elk.layered.spacing.edgeNodeBetweenLayers': '22',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
    },
    children,
    edges: elkEdges,
  };
  const laid = await new Elk().layout(root);
  const width = Math.ceil(laid.width ?? 800);
  const height = Math.ceil(laid.height ?? 400);

  const out: string[] = [];
  out.push(
    `<svg class="spine" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" style="${view === 'full' ? `width:${width}px;max-width:none` : `max-width:${width}px`}" role="img" aria-label="Flowchart of ${esc(g.flow.title ?? g.flow.id)}${view === 'main' ? ' without failure paths' : ' with failure paths'}. The text sections of this page describe every step.">`,
  );
  out.push('<defs>');
  for (const kind of ['user', 'system', 'route', 'fail', 'main']) {
    out.push(`<marker id="arrow-${view}-${kind}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path class="arrow arrow-${kind}" d="M0 0 L10 5 L0 10 z"/></marker>`);
  }
  out.push('</defs>');

  const byId = new Map<string, VizEdge>();
  edges.forEach((e, i) => byId.set(`e${i}`, e));
  const drawn = (laid.edges ?? []).filter((e) => e.sections?.[0] !== undefined);
  const isMain = (e: VizEdge | undefined): boolean => e !== undefined && mainEdges.has(`${e.from}>${e.to}`);
  const path = (e: ElkExtendedEdge): string => {
    const section = e.sections?.[0];
    if (section === undefined) return '';
    const pts = [section.startPoint, ...(section.bendPoints ?? []), section.endPoint];
    return pts.map((p, i) => `${i === 0 ? 'M' : 'L'}${r1(p.x)} ${r1(p.y)}`).join(' ');
  };
  for (const e of drawn) {
    if (e.id === 'e_start') out.push(`<path class="edge edge-system" d="${path(e)}" marker-end="url(#arrow-${view}-system)"/>`);
  }
  for (const pass of [false, true]) {
    for (const e of drawn) {
      const viz = byId.get(e.id);
      if (viz === undefined || isMain(viz) !== pass) continue;
      const kind = pass ? 'main' : viz.kind;
      out.push(`<path class="edge edge-${kind}${viz.unused ? ' edge-unused' : ''}" d="${path(e)}" marker-end="url(#arrow-${view}-${kind})"/>`);
    }
  }
  for (const e of drawn) {
    const viz = byId.get(e.id);
    const label = e.labels?.[0];
    if (viz === undefined || label === undefined) continue;
    const lines = labelLines(viz, view);
    const x = label.x ?? 0;
    const y = label.y ?? 0;
    out.push(`<g class="elabel${isMain(viz) ? ' elabel-main' : ''}${viz.kind === 'fail' ? ' elabel-fail' : ''}"><rect x="${r1(x)}" y="${r1(y)}" width="${label.width ?? 0}" height="${label.height ?? 0}" rx="4"/>`);
    lines.forEach((l, i) => out.push(`<text x="${r1(x + 7)}" y="${r1(y + 17 + i * 17)}">${esc(l)}</text>`));
    out.push('</g>');
  }
  for (const n of laid.children ?? []) {
    const x = r1(n.x ?? 0);
    const y = r1(n.y ?? 0);
    const w = n.width ?? 0;
    const h = n.height ?? 0;
    if (n.id === START) {
      out.push(`<circle class="start" cx="${r1(x + 7)}" cy="${r1(y + 7)}" r="7"><title>start</title></circle>`);
      continue;
    }
    const kind = kindOf(g, n.id);
    const st = g.flow.states[n.id];
    const exitClass = kind === 'exit' ? ` exit-${st.leavesPage === true ? 'left' : (st.redirect ?? 'error')}` : '';
    const unreachable = !ex.atState.has(n.id);
    const open = kind === 'screen' ? `<a href="#screen-${esc(n.id)}">` : '';
    const close = kind === 'screen' ? '</a>' : '';
    const by = st.by === undefined ? '' : ` by-${st.by}`;
    const tip = [st.description ?? n.id, ...(st.includes ?? []).map((s) => `+ ${s}`)].join('\n');
    out.push(`${open}<g class="node node-${kind}${by}${st.goal === true ? ' goal' : ''}${exitClass}${mainNodes.has(n.id) ? ' on-main' : ''}${unreachable ? ' node-unreachable' : ''}"><title>${esc(tip)}</title>`);
    const rx = kind === 'exit' || st.by === 'ai' ? 28 : st.by === 'ci' ? 0 : 8;
    out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}"/>`);
    if (st.by === 'agent') out.push(`<rect class="inner" x="${r1(x + 4)}" y="${r1(y + 4)}" width="${w - 8}" height="${h - 8}" rx="5"/>`);
    if ((st.includes ?? []).length > 0) out.push(`<text class="nmore" x="${r1(x + w - 8)}" y="${r1(y + 14)}" text-anchor="end">+${st.includes?.length}</text>`);
    if (kind === 'subflow') out.push(`<rect class="inner" x="${r1(x + 4)}" y="${r1(y + 4)}" width="${w - 8}" height="${h - 8}" rx="5"/>`);
    out.push(`<text class="nname" x="${r1(x + w / 2)}" y="${r1(y + 25)}" text-anchor="middle">${esc(titleOf(g.flow, n.id))}</text>`);
    out.push(`<text class="nline" x="${r1(x + w / 2)}" y="${r1(y + 44)}" text-anchor="middle">${esc(nodeLine(g, n.id))}${unreachable ? ' · unreachable' : ''}</text>`);
    out.push(`</g>${close}`);
  }
  out.push('</svg>');
  return out.join('\n');
}
