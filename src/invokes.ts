// What a flow invokes, held to its domain (SPEC.md §2, §5): `invoke: { action }` names an action of the domain the flow
// names, and for a state that invokes an action, `failures.<state>.from` does too and each class under it lists
// refusals of that action. A state that invokes an `external` or a `flow` is not held to the domain. The domain is looked
// up by id among the graphs beside the flow, all its files read as one, as the `changes` rule finds it. Checking
// `invoke` names against the domain is still open in SPEC.md (#5), so these are warnings, which fail no run.
import { flowDomain } from './changes.ts';
import type { Domain } from './changes.ts';
import type { Graph } from './graph.ts';

export interface InvokeProblem {
  rule: 'invoke-unknown' | 'refusal-unknown';
  path: string[]; // where in flow.yaml
  message: string;
  element: string;
}

interface DomainActions {
  actions?: Record<string, { refusals?: Record<string, unknown> | null } | null>;
}

// the key that names a class's action in `failures.<state>`; every other key is a class
const FROM = 'from';

// `domainOf` finds the flow's domain, as for checkChanges
export function checkInvokes(g: Graph, domainOf: () => Domain | null = () => flowDomain(g)): InvokeProblem[] {
  const invoked = Object.entries(g.flow.states).filter(([, st]) => typeof st.invoke?.action === 'string');
  if (g.flow.domain === undefined || invoked.length === 0) return [];
  const failures = ((g.flow as { failures?: unknown }).failures ?? {}) as Record<string, Record<string, unknown> | null>;
  const domain = domainOf() as DomainActions | null;
  if (domain === null) {
    return [
      {
        rule: 'invoke-unknown',
        path: ['domain'],
        element: 'domain',
        message: `the flow names the domain ${g.flow.domain}, which no domain beside it is (under the nearest folder called graphs), so what its steps invoke is not checked (SPEC.md §5)`,
      },
    ];
  }
  const actions = domain.actions ?? {};
  const has = (name: string): boolean => Object.hasOwn(actions, name);
  const out: InvokeProblem[] = [];
  for (const [state, st] of invoked) {
    const action = st.invoke?.action as string;
    if (has(action)) continue;
    out.push({ rule: 'invoke-unknown', path: ['states', state, 'invoke', 'action'], element: `states/${state}/invoke`, message: `state ${state} invokes ${action}, which is not an action of the domain ${g.flow.domain} (SPEC.md §5)` });
  }
  for (const [state, classes] of Object.entries(failures)) {
    const action = g.flow.states[state]?.invoke?.action;
    if (classes === null || typeof classes !== 'object' || typeof action !== 'string') continue;
    const named = typeof classes[FROM] === 'string' ? (classes[FROM] as string) : action;
    if (!has(named)) {
      // an action the state's own invoke names is reported there
      if (named !== action) out.push({ rule: 'invoke-unknown', path: ['failures', state, FROM], element: `failures/${state}/from`, message: `failures.${state} comes from ${named}, which is not an action of the domain ${g.flow.domain} (SPEC.md §2, §5)` });
      continue;
    }
    const refusals = actions[named]?.refusals ?? {};
    for (const [cls, codes] of Object.entries(classes)) {
      if (cls === FROM || !Array.isArray(codes)) continue;
      codes.forEach((code, i) => {
        if (typeof code !== 'string' || Object.hasOwn(refusals, code)) return;
        out.push({
          rule: 'refusal-unknown',
          path: ['failures', state, cls, String(i)],
          element: `failures/${state}/${cls}/${code}`,
          message: `FAIL.${cls} of ${state} stands for ${code}, which is not a refusal of ${named} in the domain ${g.flow.domain} (SPEC.md §2, §5)`,
        });
      });
    }
  }
  return out;
}
