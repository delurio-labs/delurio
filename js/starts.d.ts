// types only, for html.d.ts: what a flow hands over when it starts another (the module is internal to the checker)
export interface HandOver {
    from: string;
    event: string | null;
    flow: string;
    given: Record<string, string | number | boolean>;
    found: boolean;
    problems: string[];
}
