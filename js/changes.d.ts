// types only, for html.d.ts: the domain a flow names (the module is internal to the checker)
export interface DomainAction {
    route?: string;
    serverFn?: string;
    result?: {
        returns?: string;
    };
    sideEffects?: unknown[];
}
export interface Domain {
    id?: string;
    actions?: Record<string, DomainAction | null>;
    policies?: Array<{
        do?: string[];
        write?: string[];
    }>;
}
