export type Scalar = string | number | boolean;
export interface Transition {
    target: string;
    guard?: string;
    effect?: string[];
    say?: string;
    starts?: Starts;
}
export interface Starts {
    flow: string;
    given?: Record<string, string | number | boolean>;
}
export type TransitionSpec = string | Transition | Array<string | Transition>;
export interface State {
    title?: string;
    description?: string;
    type?: string;
    invoke?: {
        action?: string;
        flow?: string;
        external?: string;
    };
    on?: Record<string, TransitionSpec>;
    always?: Transition[];
    redirect?: string;
    code?: string;
    carries?: string[];
    leavesPage?: boolean;
    by?: 'human' | 'ai' | 'agent' | 'ci';
    goal?: boolean;
    includes?: string[];
    response?: Record<string, unknown>;
    states?: unknown;
    source?: string;
}
export interface ContextVar {
    values: Scalar[];
    initial: 'any' | Scalar;
    say?: Record<string, string>;
}
export interface Effect {
    set?: Record<string, Scalar>;
    havoc?: string[];
}
export interface Decision {
    question: string;
    recommend: string;
    affects: string[];
}
export interface Flow {
    id: string;
    title?: string;
    status?: string;
    summary?: string;
    decisions?: Record<string, Decision>;
    context: Record<string, ContextVar>;
    guards: Record<string, string>;
    effects: Record<string, Effect>;
    initial: string;
    states: Record<string, State>;
    assertions?: Array<{
        id: string;
        never: string;
        say?: string;
    }>;
    main?: string[] | {
        given?: Record<string, Scalar>;
        events: string[];
    };
    scope?: {
        shows?: string;
        world?: 'open' | 'closed';
        outOfView?: string[];
    };
    domain?: string;
}
export interface Label {
    key?: string;
    params?: Record<string, string>;
    labelledBy?: string;
    literal?: string;
    oneOf?: string[];
}
export interface UiNode {
    type: string;
    component?: string;
    role?: string;
    intent?: string;
    name?: Label;
    text?: Label;
    in?: string[];
    unless?: string[];
    when?: string;
    disabledIn?: string[];
    disabledWhen?: string;
    emits?: string;
    initialFocus?: string;
    repeat?: string;
    frame?: string;
    value?: Label;
    holds?: string;
    checked?: boolean | string;
    script?: 'optional' | 'actions';
    source?: string;
}
export interface UiEdge {
    labelledBy?: [string, string];
    triggers?: [string, string];
    returnsFocus?: [string, string | string[]];
}
export declare const OPENER = "opener";
export declare const CHECKABLE: readonly ['checkbox', 'menuitemcheckbox', 'menuitemradio', 'option', 'radio', 'switch'];
export declare const isDialog: (n: UiNode | undefined) => boolean;
export interface Ui {
    absent?: boolean;
    id: string;
    catalog: string;
    kit?: string;
    samples: Record<string, string>;
    nodes: Record<string, UiNode>;
    locales?: string[];
    changes?: {
        key: string;
        missing?: undefined;
    } | {
        missing: string;
        key?: undefined;
    };
    task?: {
        key: string;
    };
    focus?: Record<string, string>;
    edges?: UiEdge[];
}
export interface Graph {
    dir: string;
    flow: Flow;
    ui: Ui;
    catalog: Record<string, string>;
}
export interface ErrorPlace {
    file: string;
    path?: string[];
    line?: number;
    column?: number;
}
export declare class GraphError extends Error {
    readonly place: ErrorPlace | undefined;
    constructor(message: string, place?: ErrorPlace);
}
export declare function placed<E>(err: E, place: ErrorPlace): E;
export declare function transitionPath(state: string, st: State, event: string | null, i: number): string[];
export declare function humanize(id: string): string;
export declare function titleOf(flow: Flow, state: string): string;
export declare function shownIn(n: UiNode, state: string): boolean;
export declare function maskedNodes(g: Graph): string[];
export interface FocusAfter {
    node: string;
    by: 'returnsFocus' | 'initialFocus' | 'focus';
    from?: string;
}
export declare function focusAfter(ui: Ui, from: string | null, to: string, pressed?: string): FocusAfter | null;
export declare function alternatives(spec: TransitionSpec): Transition[];
export declare function load(dir: string, read?: (file: string) => string, exists?: (file: string) => boolean): Graph;
