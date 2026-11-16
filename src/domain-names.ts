// The names a domain gives one another (SPEC.md §5), each held to what the domain has, all its files read as one: an
// action's `returns` and every `references` (on an entity's field, or in a body, a query, a result or a refusal's body)
// name an entity; an entity's `key` and `required` name its own fields; a policy's `do` names actions, and its `read`
// and `write` entities. A name that the domain does not have is an error, as a flow's `invoke` names are (decided on
// 16 Nov 2026, d-1116-10). Each finding is given in the file that gives the name, at its line, with a note where another
// domain beside it has the entity: a domain names only its own, and one domain may be split across files instead.
import { readYaml, keyAt } from './yaml.ts';
import type { DomainPart } from './domain.ts';

export interface NamePlace {
  file: string;
  line: number;
  column: number;
}

export interface NameProblem extends NamePlace {
  rule: 'entity-unknown' | 'field-unknown' | 'policy-unknown';
  message: string;
  element: string; // where in the domain, by names, not by file or line: entities/Consent/fields/clientId/references
  notes: Array<NamePlace & { message: string }>;
}

// another domain that has an entity of this name, and where it gives it
export type EntityElsewhere = (entity: string) => (NamePlace & { domain: string }) | null;

const isObject = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v);

// the keywords of JSON Schema 2020-12 whose value is a schema, a list of schemas, or schemas by name; a value of any
// other keyword (`examples`, `default`, `const`, `enum`) is data, never read for `references`
const ONE = ['items', 'contains', 'not', 'if', 'then', 'else', 'additionalProperties', 'propertyNames', 'unevaluatedItems', 'unevaluatedProperties', 'contentSchema'];
const LIST = ['allOf', 'anyOf', 'oneOf', 'prefixItems'];
const BY_NAME = ['properties', 'patternProperties', '$defs', 'dependentSchemas'];

// every `references` in a schema, with the path to the schema that gives it
function referencesIn(schema: unknown, path: string[], out: Array<{ path: string[]; name: unknown }> = []): Array<{ path: string[]; name: unknown }> {
  if (!isObject(schema)) return out;
  if (schema.references !== undefined) out.push({ path, name: schema.references });
  for (const k of ONE) referencesIn(schema[k], [...path, k], out);
  for (const k of LIST) if (Array.isArray(schema[k])) (schema[k] as unknown[]).forEach((s, i) => referencesIn(s, [...path, k, String(i)], out));
  for (const k of BY_NAME) if (isObject(schema[k])) for (const [name, s] of Object.entries(schema[k])) referencesIn(s, [...path, k, name], out);
  return out;
}

// a schema's path inside the value it sits in, for a message: properties.address.properties.city -> address.city
const inside = (path: string[]): string => path.map((s, i) => (path[i - 1] === 'properties' ? s : s === 'properties' ? '' : s)).filter((s) => s !== '').join('.');

// `doc` is the domain's files read as one (mergeDomain); `parts` the files, each read again here for where it gives a name
export function checkDomainNames(parts: DomainPart[], doc: Record<string, unknown>, id: string, elsewhere: EntityElsewhere = () => null): NameProblem[] {
  const entities = isObject(doc.entities) ? doc.entities : {};
  const actions = isObject(doc.actions) ? doc.actions : {};
  const isEntity = (name: string): boolean => Object.hasOwn(entities, name);
  const isAction = (name: string): boolean => Object.hasOwn(actions, name);
  const fieldsOf = (entity: string): Record<string, unknown> | null => {
    const e = entities[entity];
    return isObject(e) && isObject(e.fields) ? e.fields : null;
  };
  const out: NameProblem[] = [];
  for (const part of parts) {
    let data: unknown;
    try {
      data = readYaml(part.text);
    } catch {
      continue;
    }
    // a file that is not a map, or is another domain's (domain-place), is no part of this one
    if (!isObject(data) || (typeof data.id === 'string' && data.id !== id)) continue;
    const at = (rule: NameProblem['rule'], path: string[], message: string, element: string, entity?: string): void => {
      const other = entity === undefined ? null : elsewhere(entity);
      const notes = other === null ? [] : [{ file: other.file, line: other.line, column: other.column, message: `${entity} is an entity of the domain ${other.domain}: a domain names only its own entities, and one domain may be split across the files of its directory (SPEC.md §5)` }];
      out.push({ rule, file: part.file, ...keyAt(part.text, path), message, element, notes });
    };
    const references = (schema: unknown, path: string[], where: (rest: string) => string): void => {
      for (const r of referencesIn(schema, path)) {
        if (typeof r.name !== 'string' || isEntity(r.name)) continue;
        const rest = inside(r.path.slice(path.length));
        at('entity-unknown', [...r.path, 'references'], `${where(rest)} references ${r.name}, which is not an entity of the domain ${id} (SPEC.md §5)`, `${r.path.join('/')}/references`, r.name);
      }
    };

    for (const [entity, e] of Object.entries(isObject(data.entities) ? data.entities : {})) {
      if (!isObject(e)) continue;
      // an entity whose fields no file gives yet is not held to them
      const fields = fieldsOf(entity);
      for (const key of ['key', 'required'] as const) {
        if (fields === null || !Array.isArray(e[key])) continue;
        (e[key] as unknown[]).forEach((name, i) => {
          if (typeof name !== 'string' || Object.hasOwn(fields, name)) return;
          const said = key === 'key' ? `the key of the entity ${entity} names ${name}` : `the entity ${entity} requires ${name}`;
          at('field-unknown', ['entities', entity, key, String(i)], `${said}, which is not one of its fields (SPEC.md §5)`, `entities/${entity}/${key}/${name}`);
        });
      }
      for (const [field, schema] of Object.entries(isObject(e.fields) ? e.fields : {})) {
        references(schema, ['entities', entity, 'fields', field], (rest) => `the field ${entity}.${field}${rest === '' ? '' : `, at ${rest},`}`);
      }
    }

    for (const [action, a] of Object.entries(isObject(data.actions) ? data.actions : {})) {
      if (!isObject(a)) continue;
      const result = a.result;
      if (isObject(result) && typeof result.returns === 'string' && !isEntity(result.returns)) {
        at('entity-unknown', ['actions', action, 'result', 'returns'], `the action ${action} returns ${result.returns}, which is not an entity of the domain ${id} (SPEC.md §5)`, `actions/${action}/result/returns`, result.returns);
      }
      for (const section of ['query', 'body', 'result'] as const) {
        references(a[section], ['actions', action, section], (rest) => `the ${section} of the action ${action}${rest === '' ? '' : `, at ${rest},`}`);
      }
      for (const [code, r] of Object.entries(isObject(a.refusals) ? a.refusals : {})) {
        if (isObject(r)) references(r.body, ['actions', action, 'refusals', code, 'body'], (rest) => `the body of the refusal ${code} of the action ${action}${rest === '' ? '' : `, at ${rest},`}`);
      }
    }

    // the policies of this file, by their place in it; the element names the policy by its actor, as the list's order
    // across files is no part of what it is
    (Array.isArray(data.policies) ? data.policies : []).forEach((p: unknown, i: number) => {
      if (!isObject(p)) return;
      const actor = typeof p.actor === 'string' ? p.actor : String(i);
      for (const key of ['do', 'read', 'write'] as const) {
        if (!Array.isArray(p[key])) continue;
        (p[key] as unknown[]).forEach((name, j) => {
          if (typeof name !== 'string') return;
          const known = key === 'do' ? isAction(name) : isEntity(name);
          if (known) return;
          const what = key === 'do' ? 'an action' : 'an entity';
          at('policy-unknown', ['policies', String(i), key, String(j)], `the policy for ${actor} names ${name} in ${key}, which is not ${what} of the domain ${id} (SPEC.md §5)`, `policies/${actor}/${key}/${name}`, key === 'do' ? undefined : name);
        });
      }
    });
  }
  return out;
}
