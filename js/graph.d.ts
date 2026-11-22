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
    /** Values the variables are given; `row.<field>` is a field of the row pressed, which the checker lets be any value of the variable. */
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
    native?: 'date' | 'time' | 'datetime-local' | 'month' | 'week' | 'file' | 'color' | 'summary';
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
    /** Drawn once per item of this data: once per row the samples give it, else once. */
    repeat?: string;
    frame?: string;
    value?: Label;
    holds?: string;
    /** Whether it is checked: an expression over the context (on a repeat node, also over its row, `row.<field>`), or true or false. */
    checked?: boolean | string;
    script?: 'optional' | 'actions';
    source?: string;
    /** How the node looks, never all it says (SPEC.md §4): a fixed tone, or each tone with the expression over the context under which the node takes it; the first that holds wins. */
    tone?: Tone | Partial<Record<Tone, string>>;
}
/** The tones a node may take (SPEC.md §4). */
export type Tone = 'neutral' | 'info' | 'success' | 'warning' | 'danger';
/** One item of a repeat's data in the samples: its fields and their values (SPEC.md §4). */
export type SampleRow = Record<string, Scalar>;
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
    /** A sample value for each catalog parameter, or, under a repeat's data (`question.options`), its rows: each a map of the fields its nodes read as `row.<field>`. */
    samples: Record<string, string | SampleRow[]>;
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
export type Sensitivity = 'credential' | 'personal' | 'internal';
export declare function maskedNodes(g: Graph, mask?: readonly Sensitivity[]): string[];
export interface FocusAfter {
    node: string;
    by: 'returnsFocus' | 'initialFocus' | 'focus';
    from?: string;
}
export declare function focusAfter(ui: Ui, from: string | null, to: string, pressed?: string): FocusAfter | null;
export declare function alternatives(spec: TransitionSpec): Transition[];
export declare function load(dir: string, read?: (file: string) => string, exists?: (file: string) => boolean): Graph;
