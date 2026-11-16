export type Value = string | number | boolean | null | Value[];
export type Expr = {
    k: 'lit';
    v: Value;
} | {
    k: 'id';
    name: string;
} | {
    k: 'list';
    items: Expr[];
} | {
    k: 'not';
    e: Expr;
} | {
    k: 'and' | 'or';
    l: Expr;
    r: Expr;
} | {
    k: 'cmp';
    op: '==' | '!=' | 'in' | 'not in';
    l: Expr;
    r: Expr;
};
export declare function parseExpr(src: string): Expr;
export declare function evaluate(e: Expr, lookup: (name: string) => Value): Value;
export declare function identifiers(e: Expr, out?: Set<string>): Set<string>;
