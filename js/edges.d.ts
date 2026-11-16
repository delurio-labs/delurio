import type { Graph, State } from './graph.js';
import type { Exploration } from './model.js';
export type EdgeKind = 'user' | 'system' | 'route' | 'fail';
export interface VizEdge {
    from: string;
    to: string;
    labels: string[];
    kind: EdgeKind;
    leave: boolean;
    unused: boolean;
}
export type NodeKind = 'server' | 'screen' | 'subflow' | 'exit';
export declare const BY: Record<string, string>;
export declare function kindOf(g: Graph, name: string): NodeKind;
export declare function exitLine(st: State): string;
export declare function responseLine(r: Record<string, unknown>): string;
export declare function nodeLine(g: Graph, name: string): string;
export declare function plain(g: Graph, key: string): string;
export declare function pressLabel(g: Graph, state: string, event: string): string | null;
export declare function wrap(text: string, max: number): string[];
export declare function collectEdges(g: Graph, ex: Exploration): VizEdge[];
