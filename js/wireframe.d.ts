import type { Graph } from './graph.js';
import type { Kit } from './kit.js';
type Warn = (message: string) => void;
export declare function renderScreens(g: Graph, warn: Warn, kit?: Kit | null): string;
export declare function renderStageScreen(g: Graph, state: string, reveal: string[], warn: Warn, kit?: Kit | null): string;
export interface Expectation {
    state: string;
    id: string;
    index: number;
    role: string;
    /** False when ui.yaml gives a control (a field or an action) no role: `role` is then its type, which no element is. */
    roleGiven: boolean;
    name: string | undefined;
    disabled: boolean;
}
export declare function renderCheckScreens(g: Graph, kit: Kit | null, warn: Warn): {
    html: string;
    expected: Expectation[];
};
export declare function nodeTexts(g: Graph, id: string, warn: Warn): string[];
export {};
