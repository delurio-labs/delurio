import type { Value } from './expr.js';
export type PageKind = 'screen' | 'placeholder' | 'missing' | 'redirect' | 'external';
export type FrameId = string;
export type How = 'auto' | 'menu' | 'picker' | 'user' | 'link' | 'button';
export interface FrameSpec {
    title: string;
    menu?: string;
    shows?: {
        fact: string;
    };
}
interface FactSpec {
    values?: Value[];
    of?: string[];
    say?: string;
}
interface EntrySpec {
    id?: string;
    from: string | string[];
    to: string;
    how: string;
    label?: string;
    when?: string;
    clicks?: number;
    data?: boolean;
    back?: boolean;
}
export interface ExtractCfg {
    adapter?: string;
    names?: Record<string, string>;
    titles?: Record<string, string>;
    locales?: string[];
    entries?: Record<string, {
        when?: string;
        data?: boolean;
        label?: string;
        how?: string;
        back?: boolean;
        clicks?: number;
        skip?: string;
    }>;
}
export interface NavSource {
    repo?: string;
    commit?: string;
    app?: string;
    menu?: string;
    locales?: string | string[];
    [file: string]: string | string[] | undefined;
}
export interface Page {
    id: string;
    route: string;
    title: string;
    titleKey: string | null;
    kind: PageKind;
    frame: FrameId;
    source: string | null;
    opensFor: string | null;
    pageCodes: string | null;
    evidence: string | null;
    refusal: string | null;
    byAddress: boolean;
}
export interface Entry {
    id: string;
    spec: number;
    from: string;
    ofFrame: FrameId | null;
    to: string;
    how: How;
    label: string;
    labelKey: string | null;
    when: string | null;
    clicks: number;
    data: boolean;
    back: boolean;
    order: number;
}
export interface Persona {
    id: string;
    title: string;
    say: string;
    facts: Record<string, Value>;
}
export interface Goal {
    id: string;
    say: string;
    page: string;
    for: string[];
    within: number;
}
export interface NavGraph {
    dir: string;
    adapter: string | null;
    frames: Record<string, FrameSpec>;
    menuFrames: string[];
    id: string;
    title: string;
    status: string;
    summary: string;
    locale: string;
    source: NavSource;
    facts: Record<string, FactSpec>;
    guards: Record<string, string>;
    pages: Record<string, Page>;
    entries: Entry[];
    specs: EntrySpec[];
    personas: Persona[];
    goals: Goal[];
    notModeled: string[];
    catalog: Record<string, string>;
}
export declare const frameLevel: (e: Entry) => boolean;
export declare function loadNav(dir: string, locale?: string, read?: (file: string) => string): {
    g: NavGraph;
    warnings: string[];
};
export declare function holds(g: NavGraph, p: Persona, expr: string | null): boolean;
export declare const refusedFor: (g: NavGraph, p: Persona, page: string) => boolean;
export interface Reach {
    page: string;
    clicks: number;
    via: Entry | null;
    prev: string | null;
    usesData: boolean;
    byAddress: string | null;
}
export declare function walk(g: NavGraph, p: Persona): Map<string, Reach>;
export declare function pathTo(reach: Map<string, Reach>, page: string): Entry[];
export type Standing = 'ok' | 'placeholder' | 'missing' | 'refused' | 'unreachable';
export interface PersonaView {
    persona: Persona;
    reach: Map<string, Reach>;
    standing: Record<string, Standing>;
    landing: string | null;
}
export declare function viewOf(g: NavGraph, p: Persona): PersonaView;
export declare const frameReached: (g: NavGraph, v: PersonaView, frame: FrameId) => boolean;
export interface MenuCell {
    shown: boolean;
    standing: Standing | null;
}
export interface MenuRow {
    spec: number;
    frame: FrameId;
    entry: Entry;
    cells: MenuCell[];
    frameKnown: boolean[];
}
export declare function menuRows(g: NavGraph, views: PersonaView[]): MenuRow[];
export interface Finding {
    level: 'problem' | 'note';
    code: string;
    say: string;
    detail: string[];
}
export interface Goalcheck {
    goal: Goal;
    persona: Persona;
    outcome: 'ok' | 'too many clicks' | 'not built' | 'does not exist' | 'refused' | 'cannot reach';
    clicks: number | null;
}
export declare function checkGoals(g: NavGraph, views: PersonaView[]): Goalcheck[];
export declare function findings(g: NavGraph, views: PersonaView[], goals: Goalcheck[]): Finding[];
export interface RepoCheck {
    ok: boolean;
    what: string;
    detail: string;
}
export declare function guardCodes(g: NavGraph, src: string, depth?: number): Set<string>;
export {};
