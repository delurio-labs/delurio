export declare const NEEDS_FILE = "needs.yaml";
export declare const GAPS_FILE = "needs.gaps.yaml";
export interface Evidence {
    source?: string;
    test?: string;
    name?: string;
    scenario?: string;
    binding?: string;
    state?: string;
    node?: string;
    setting?: string;
    in?: string;
}
export interface Need {
    id: string;
    need: string;
    evidence?: Evidence[];
    missing?: string;
    note?: string;
}
export interface Needs {
    schema?: string;
    kind: 'needs';
    id: string;
    status?: string;
    needs: Need[];
}
export type NeedRule = 'evidence-unresolved' | 'need-no-evidence' | 'need-names-built' | 'need-unlisted' | 'need-gap' | 'need-twice' | 'needs-place' | 'gap-met' | 'gap-unknown';
export interface NeedProblem {
    rule: NeedRule;
    file: string;
    line: number;
    column: number;
    message: string;
    graph: string;
    element: string;
}
export interface NeedsCheck {
    problems: NeedProblem[];
    files: number;
    needs: number;
    gaps: number;
    notLookedFor: number;
}
export declare function readNeeds(dir: string): Needs | null;
export interface Planned {
    states: Map<string, string[]>;
    nodes: Map<string, string[]>;
}
export declare function planned(needs: Needs | null): Planned;
export declare function checkNeeds(files: string[], app: string | null): NeedsCheck;
