export declare class KitError extends Error {
}
declare const SLOTS: readonly ['text', 'name', 'id', 'intent', 'children', 'tone'];
declare const FLAGS: readonly ['disabled', 'named', 'conditional', 'checked'];
type Slot = (typeof SLOTS)[number];
type Flag = (typeof FLAGS)[number];
type Part = {
    lit: string;
} | {
    slot: Slot;
    ctx: 'text' | 'dq' | 'sq';
} | {
    flag: Flag;
    negate: boolean;
    parts: Part[];
};
export interface Template {
    source: string;
    parts: Part[];
}
export interface KitComponent {
    template: Template;
    roles: Record<string, Template>;
    intents?: Record<string, string>;
    /** node tone -> the word {tone} gives, read before the kit's own `tones`; `default` for a node without one */
    tones?: Record<string, string>;
}
/** A font the kit carries (`fonts:` in kit.yaml, SPEC.md §8). */
export interface KitFont {
    family: string;
    /** as kit.yaml gives it, relative to the kit's folder */
    file: string;
    weight?: string;
    style?: 'normal' | 'italic' | 'oblique';
    unicodeRange?: string;
    /** the file as a data: URL; absent when it cannot be embedded */
    src?: string;
}
export interface Kit {
    id: string;
    title: string;
    file: string;
    components: Record<string, KitComponent>;
    light: Array<[string, string]>;
    dark: Array<[string, string]>;
    css: string;
    zoom: number;
    /** node tone -> the word {tone} gives, for a component that gives none of its own; `default` for a node without one */
    tones?: Record<string, string>;
    /** the fonts it carries, in the order of kit.yaml */
    fonts?: KitFont[];
}
export interface Fill {
    id: string;
    text: string;
    name: string;
    intent: string | undefined;
    role: string | undefined;
    children?: string;
    disabled: boolean;
    named: boolean;
    conditional: boolean;
    checked: boolean;
    /** the tone the node is drawn in (SPEC.md §4) */
    tone?: string;
}
export declare function drawWith(kit: Kit, component: string, f: Fill, warn: (m: string) => void): string | null;
export declare function mark(html: string, id: string, chips: string): string;
export declare function loadKit(path: string): Kit;
export declare function resolveKit(graphDir: string, uiKit: string | undefined, flagValue: string | undefined): Kit | null;
export declare function kitCss(kit: Kit, target: 'page' | 'stage'): string;
export declare function kitStageScript(kit: Kit): string;
/** The families of the kit's own fonts (`fonts:`), each once, in the order of kit.yaml: what its screens should be drawn in. */
export declare function kitFamilies(kit: Kit): string[];
export {};
