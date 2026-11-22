import type { Graph } from './graph.js';
import type { Kit } from './kit.js';
type Warn = (message: string) => void;
export declare function renderScreens(g: Graph, warn: Warn, kit?: Kit | null): string;
/** The context a stage screen is drawn in: each name of the flow's expressions and its value. */
export type StageContext = Record<string, string | number | boolean | null>;
/** One state's screen for the tutorial stage; `ctx` picks each node's tone from its map (SPEC.md §4), and without it only a fixed tone is drawn. */
export declare function renderStageScreen(g: Graph, state: string, reveal: string[], warn: Warn, kit?: Kit | null, ctx?: StageContext | null): string;
/** The styles of a tone drawn by the fixed components (`wf-tone-<word>`): a page adds them where a node has a tone; a video stage adds them too. */
export declare const TONE_CSS: string;
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
