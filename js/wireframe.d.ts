import type { Graph, Scalar } from './graph.js';
import type { Kit } from './kit.js';
type Warn = (message: string) => void;
export declare function renderScreens(g: Graph, warn: Warn, kit?: Kit | null): string;
/**
 * One state's screen for the tutorial stage. `ctx`, the context of the situation it shows (each variable of the flow
 * and its value), draws each node's `checked` as it reads there, for each row it is drawn for; without it only
 * `checked: true` is drawn.
 */
export declare function renderStageScreen(g: Graph, state: string, reveal: string[], warn: Warn, kit?: Kit | null, ctx?: Record<string, Scalar | null> | null): string;
export interface Expectation {
    state: string;
    id: string;
    index: number;
    role: string;
    /** False when ui.yaml gives a control (a field or an action) no role: `role` is then its type, which no element is. */
    roleGiven: boolean;
    /** The native control a field or an action with no role says it is (`native: date`): found by its name among controls of that kind. */
    native?: string;
    name: string | undefined;
    disabled: boolean;
}
export declare function renderCheckScreens(g: Graph, kit: Kit | null, warn: Warn): {
    html: string;
    expected: Expectation[];
};
export declare function nodeTexts(g: Graph, id: string, warn: Warn): string[];
export {};
