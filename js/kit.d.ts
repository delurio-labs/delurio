export declare class KitError extends Error {
}
declare const SLOTS: readonly ['text', 'name', 'id', 'intent', 'children'];
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
}
export declare function drawWith(kit: Kit, component: string, f: Fill, warn: (m: string) => void): string | null;
export declare function mark(html: string, id: string, chips: string): string;
export declare function loadKit(path: string): Kit;
export declare function resolveKit(graphDir: string, uiKit: string | undefined, flagValue: string | undefined): Kit | null;
export declare function kitCss(kit: Kit, target: 'page' | 'stage'): string;
export declare function kitStageScript(kit: Kit): string;
export {};
