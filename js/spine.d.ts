import type { Graph } from './graph.js';
import type { Exploration } from './model.js';
import type { PathStep } from './story.js';
export type View = 'main' | 'full';
/**
 * The flow's diagram as an SVG. Every state's group carries `data-state`, and every edge and its label `data-from` and
 * `data-to`: one edge is drawn per pair of states, its label holding every transition between them. `story`: the
 * states to draw as the story (`on-main`), in order.
 */
export declare function renderSpine(g: Graph, ex: Exploration, view: View, story: PathStep[] | null): Promise<string>;
