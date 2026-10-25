// The guard and assertion language: a small, fixed subset, interpreted without eval.
//   or / and / not, == / != / in / not in, parentheses, [lists], 'strings', numbers, true / false / null, names.
export type Value = string | number | boolean | null | Value[];

export type Expr =
  | { k: 'lit'; v: Value }
  | { k: 'id'; name: string }
  | { k: 'list'; items: Expr[] }
  | { k: 'not'; e: Expr }
  | { k: 'and' | 'or'; l: Expr; r: Expr }
  | { k: 'cmp'; op: '==' | '!=' | 'in' | 'not in'; l: Expr; r: Expr };

const cache = new Map<string, Expr>();

function tokenize(src: string): string[] {
  const out: string[] = [];
  const re = /\s*(?:==|!=|[()[\],]|'[^']*'|"[^"]*"|-?\d+(?:\.\d+)?|[A-Za-z_][A-Za-z0-9_.]*)/y;
  let i = 0;
  while (i < src.length) {
    re.lastIndex = i;
    const m = re.exec(src);
    if (m === null) {
      if (src.slice(i).trim() === '') break;
      throw new Error(`cannot read "${src.slice(i)}" in: ${src}`);
    }
    out.push(m[0].trim());
    i = re.lastIndex;
  }
  return out;
}

export function parseExpr(src: string): Expr {
  const hit = cache.get(src);
  if (hit !== undefined) return hit;
  const toks = tokenize(src);
  let i = 0;
  const peek = (): string | undefined => toks[i];
  const next = (): string | undefined => toks[i++];
  const expect = (t: string): void => {
    if (next() !== t) throw new Error(`expected "${t}" in: ${src}`);
  };

  const atom = (): Expr => {
    const t = next();
    if (t === undefined) throw new Error(`unexpected end in: ${src}`);
    if (t === '(') {
      const e = orExpr();
      expect(')');
      return e;
    }
    if (t === '[') {
      const items: Expr[] = [];
      while (peek() !== ']') {
        items.push(atom());
        if (peek() === ',') next();
      }
      expect(']');
      return { k: 'list', items };
    }
    if (t.startsWith("'") || t.startsWith('"')) return { k: 'lit', v: t.slice(1, -1) };
    if (/^-?\d/.test(t)) return { k: 'lit', v: Number(t) };
    if (t === 'true') return { k: 'lit', v: true };
    if (t === 'false') return { k: 'lit', v: false };
    if (t === 'null') return { k: 'lit', v: null };
    if (/^[A-Za-z_]/.test(t) && !['and', 'or', 'not', 'in'].includes(t)) return { k: 'id', name: t };
    throw new Error(`unexpected "${t}" in: ${src}`);
  };
  const cmpExpr = (): Expr => {
    const l = atom();
    const p = peek();
    if (p === '==' || p === '!=') {
      next();
      return { k: 'cmp', op: p, l, r: atom() };
    }
    if (p === 'in') {
      next();
      return { k: 'cmp', op: 'in', l, r: atom() };
    }
    if (p === 'not' && toks[i + 1] === 'in') {
      i += 2;
      return { k: 'cmp', op: 'not in', l, r: atom() };
    }
    return l;
  };
  const notExpr = (): Expr => {
    if (peek() === 'not') {
      next();
      return { k: 'not', e: notExpr() };
    }
    return cmpExpr();
  };
  const andExpr = (): Expr => {
    let l = notExpr();
    while (peek() === 'and') {
      next();
      l = { k: 'and', l, r: notExpr() };
    }
    return l;
  };
  const orExpr = (): Expr => {
    let l = andExpr();
    while (peek() === 'or') {
      next();
      l = { k: 'or', l, r: andExpr() };
    }
    return l;
  };

  const e = orExpr();
  if (i !== toks.length) throw new Error(`unexpected "${toks[i]}" in: ${src}`);
  cache.set(src, e);
  return e;
}

export function evaluate(e: Expr, lookup: (name: string) => Value): Value {
  switch (e.k) {
    case 'lit':
      return e.v;
    case 'id':
      return lookup(e.name);
    case 'list':
      return e.items.map((x) => evaluate(x, lookup));
    case 'not':
      return !evaluate(e.e, lookup);
    case 'and':
      return Boolean(evaluate(e.l, lookup)) && Boolean(evaluate(e.r, lookup));
    case 'or':
      return Boolean(evaluate(e.l, lookup)) || Boolean(evaluate(e.r, lookup));
    case 'cmp': {
      const l = evaluate(e.l, lookup);
      const r = evaluate(e.r, lookup);
      if (e.op === '==') return l === r;
      if (e.op === '!=') return l !== r;
      const inList = Array.isArray(r) && r.some((x) => x === l);
      return e.op === 'in' ? inList : !inList;
    }
  }
}

export function identifiers(e: Expr, out: Set<string> = new Set()): Set<string> {
  switch (e.k) {
    case 'id':
      out.add(e.name);
      break;
    case 'list':
      e.items.forEach((x) => identifiers(x, out));
      break;
    case 'not':
      identifiers(e.e, out);
      break;
    case 'and':
    case 'or':
    case 'cmp':
      identifiers(e.l, out);
      identifiers(e.r, out);
      break;
    case 'lit':
      break;
  }
  return out;
}
