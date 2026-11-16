import type { ErrorPlace, Flow, Scalar } from './graph.js';
import type { Value } from './expr.js';
export declare class ModelError extends Error {
    readonly place: ErrorPlace | undefined;
    constructor(message: string, place?: ErrorPlace);
}
export interface Step {
    event: string | null;
    target: string;
    ctx: Scalar[];
    taken: string;
}
export interface Config {
    state: string;
    ctx: Scalar[];
    parent: number;
    event: string | null;
}
export declare class Model {
    readonly flow: Flow;
    readonly vars: string[];
    readonly domains: Scalar[][];
    private readonly at;
    private readonly compiled;
    private readonly ready;
    private readonly ways;
    constructor(flow: Flow);
    private compile;
    private expr;
    test(expr: string, state: string, ctx: Scalar[]): boolean;
    /**
     * Whether evaluating `src` can never throw: it names only the state, variables, and guards that parse and nest no
     * deeper than evaluation allows. Only then may its pairs be tested in another order than the exploration's.
     */
    pure(src: string): boolean;
    /**
     * The value of `src` in `state` whatever the context holds, when the state alone decides it (`state in [...] and
     * ...` outside those states); undefined when the context matters. Folds only what evaluation would compute the same
     * way: literals, the state, and the side of an `and` / `or` that decides it before the other is read.
     */
    inState(src: string, state: string): Value | undefined;
    /** `test` for one expression at many pairs: read once, then run at each. */
    tester(expr: string): (state: string, ctx: Scalar[]) => boolean;
    value(expr: string, state: string, ctx: Scalar[]): Value;
    private effect;
    private apply;
    private waysOf;
    steps(state: string, ctx: Scalar[]): Step[];
    /**
     * `err`, thrown by `steps(state, ctx)`, given the place in flow.yaml of the transition that threw it when it has
     * none: the ways are tried again as `steps` tries them, and the one whose guard or effect throws is the place; else
     * the state. Read only once a step has thrown, so the exploration itself pays nothing for it.
     */
    placeError<E>(err: E, state: string, ctx: Scalar[]): E;
    initials(): Scalar[][];
    scenario(ctx: Scalar[]): Record<string, Scalar>;
}
export interface Exploration {
    configs: Config[];
    starts: number;
    taken: Set<string>;
    atState: Map<string, number>;
    branchCounts: Map<string, number[]>;
}
export declare function explore(m: Model, from?: Scalar[][], keep?: (at: Config, s: Step) => boolean): Exploration;
export interface PathResult {
    events: string[];
    start: Record<string, Scalar>;
}
export declare function pathTo(m: Model, ex: Exploration, index: number): PathResult;
export interface AssertionResult {
    id: string;
    never: string;
    say?: string;
    violation?: PathResult & {
        state: string;
    };
}
export declare function checkAssertions(m: Model, ex: Exploration): AssertionResult[];
export interface FinalPath extends PathResult {
    state: string;
    index: number;
}
export declare function finalPaths(m: Model, ex: Exploration): FinalPath[];
export interface CoverPath {
    id: string;
    start: Record<string, Scalar>;
    events: string[];
    states: string[];
    transitions: string[];
    covers: string[];
}
export interface Cover {
    by: 'states' | 'transitions';
    paths: CoverPath[];
    unreached: string[];
}
export declare function transitionsOf(flow: Flow): string[];
/**
 * Event paths that together pass every reachable state (`states`) or take every reachable transition (`transitions`)
 * of the flow (#4), from the same starts and through the same steps as `explore(m, from, keep)`. Each is a shortest
 * way to the state or transition it is first kept for, so it repeats a state only on the way to something not yet
 * covered. The longest are kept first and a path that covers nothing new is left out: a small set, never every path.
 * The same flow gives the same paths, in the same order, with the same ids.
 */
export declare function cover(m: Model, by: 'states' | 'transitions', from?: Scalar[][], keep?: (at: Config, s: Step) => boolean): Cover;
