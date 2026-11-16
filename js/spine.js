// The flow's diagram as an SVG, laid out by elkjs: the graph handed to elkjs and the SVG drawn from its layout are the
// Rust checker's (crates/delurio-page/src/spine.rs); the layout is Node's own elkjs, the one delurio's dashboard lays
// its diagrams out with, so both give the same positions.
import ElkModule from 'elkjs/lib/elk.bundled.js';
import { call, native, nativeExploration, nativeGraph } from './internal.js';

// elkjs ships CommonJS: under Node ESM the default import is module.exports, which is the constructor
const Elk = ElkModule;

/**
 * The flow's diagram as an SVG. Every state's group carries `data-state`, and every edge and its label `data-from` and
 * `data-to`: one edge is drawn per pair of states, its label holding every transition between them. `story`: the
 * states to draw as the story (`on-main`), in order.
 */
export async function renderSpine(g, ex, view, story) {
  const n = nativeGraph(g);
  const x = nativeExploration(ex);
  const states = story === null || story === undefined ? undefined : story.map((s) => String(s.state));
  const root = JSON.parse(call(null, () => native.spineGraph(n, x, String(view), states)));
  const laid = await new Elk().layout(root);
  return call(null, () => native.spineSvg(n, x, String(view), states, JSON.stringify(laid)));
}
