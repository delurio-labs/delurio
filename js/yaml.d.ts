/** the code of a YAML error, as the yaml library 2.9.1 names it */
type ErrorCode = 'ALIAS_PROPS' | 'BAD_ALIAS' | 'BAD_DIRECTIVE' | 'BAD_DQ_ESCAPE' | 'BAD_INDENT' | 'BAD_PROP_ORDER' | 'BAD_SCALAR_START' | 'BLOCK_AS_IMPLICIT_KEY' | 'BLOCK_IN_FLOW' | 'DUPLICATE_KEY' | 'IMPOSSIBLE' | 'KEY_OVER_1024_CHARS' | 'MISSING_CHAR' | 'MULTILINE_IMPLICIT_KEY' | 'MULTIPLE_ANCHORS' | 'MULTIPLE_DOCS' | 'MULTIPLE_TAGS' | 'NON_STRING_KEY' | 'RESOURCE_EXHAUSTION' | 'TAB_AS_INDENT' | 'TAG_RESOLVE_FAILED' | 'UNEXPECTED_TOKEN' | 'BAD_COLLECTION_TYPE';
export interface Unsupported {
    offset: number;
    line: number;
    column: number;
    code: ErrorCode;
    message: string;
}
export declare function lineAndColumn(text: string, offset: number): {
    line: number;
    column: number;
};
export declare function unsupported(text: string): Unsupported | null;
export declare function parsingOnce<T>(run: () => T): T;
export declare function readYaml(text: string): unknown;
export declare function keyAt(text: string, path: string[]): {
    line: number;
    column: number;
};
