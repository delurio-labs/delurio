import type { Graph, Scalar } from './graph.js';
import type { Exploration, Model } from './model.js';
export interface PathStep {
    state: string;
    ctx: Scalar[];
    event: string | null;
    taken: string | null;
}
export interface Story {
    given: string;
    steps: string[];
    result: string;
}
export declare function stepsTo(m: Model, ex: Exploration, index: number): PathStep[];
export declare function stepsByEvents(m: Model, main: NonNullable<Graph['flow']['main']>): PathStep[] | null;
export declare function mainStoryIndex(g: Graph, ex: Exploration): number | null;
export declare function narrate(g: Graph, m: Model, steps: PathStep[]): Story;
