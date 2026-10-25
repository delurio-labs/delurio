import { alternatives, humanize, titleOf } from './graph.ts';
import type { Graph } from './graph.ts';
import { identifiers, parseExpr } from './expr.ts';
import { collectEdges, exitLine, kindOf, pressLabel, responseLine } from './edges.ts';
import type { AssertionResult, Exploration, FinalPath, Model } from './model.ts';
import { narrate, stepsTo } from './story.ts';
import type { Story } from './story.ts';
import { kitCss } from './kit.ts';
import type { Kit } from './kit.ts';
import type { HandOver } from './starts.ts';

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const code = (s: string): string => `<code>${esc(s)}</code>`;

export const CSS = `
:root { --bg:#fafaf9; --surface:#ffffff; --ink:#1c1917; --muted:#6b6560; --line:#d6d3d1; --accent:#1d4ed8; --accent-bg:#eef3ff; --ok:#15803d; --bad:#b91c1c; --server:#eeeeec; --focus:#1d4ed8; --ai:#7e22ce; --ai-bg:#f5edff; --agent:#c2410c; --agent-bg:#fff1e8; --ci:#0f766e; --ci-bg:#e6f6f3; --ok-bg:#eaf7ee; }
@media (prefers-color-scheme: dark) { :root { --bg:#161513; --surface:#1f1d1b; --ink:#f2efec; --muted:#a8a29e; --line:#44403c; --accent:#8fb0ff; --accent-bg:#1c2438; --ok:#5fcf85; --bad:#f08080; --server:#2a2826; --focus:#8fb0ff; --ai:#c99bff; --ai-bg:#2a2135; --agent:#ff9a66; --agent-bg:#33241b; --ci:#5fd4c4; --ci-bg:#1a2e2b; --ok-bg:#16291d; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink); font: 16px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 1000px; margin: 0 auto; padding: 28px 16px 80px; }
h1 { font-size: 30px; line-height: 1.2; margin: 6px 0 8px; } h2 { font-size: 21px; margin: 44px 0 12px; } h3 { font-size: 16px; margin: 0; }
code, pre, .mono { font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; font-size: 13px; }
.muted { color: var(--muted); } .label { margin: 0 0 6px; font-size: 12px; text-transform: uppercase; letter-spacing: .04em; color: var(--muted); }
.eyebrow { margin: 0; color: var(--muted); } .lede { font-size: 18px; max-width: 46em; margin: 0; }
.tag { display: inline-block; padding: 1px 8px; border: 1px solid var(--line); border-radius: 99px; font-size: 12px; color: var(--muted); vertical-align: middle; }
.scroll { overflow-x: auto; border: 1px solid var(--line); border-radius: 10px; background: var(--surface); }
table { border-collapse: collapse; width: 100%; } th, td { text-align: left; vertical-align: top; padding: 8px 12px; border-bottom: 1px solid var(--line); } th { font-size: 12px; text-transform: uppercase; letter-spacing: .04em; color: var(--muted); } tr:last-child td { border-bottom: 0; }
td ol { margin: 0; padding-left: 20px; }
.ok { color: var(--ok); font-weight: 600; } .bad { color: var(--bad); font-weight: 600; }
.story { margin-top: 28px; padding: 18px 22px; border: 1px solid var(--line); border-left: 5px solid var(--accent); border-radius: 10px; background: var(--surface); }
.story h2 { margin: 0 0 6px; } .story .given { margin: 0 0 8px; } .story ol { margin: 0; padding-left: 26px; } .story li { margin: 5px 0; padding-left: 4px; } .story .result { margin: 10px 0 0; font-weight: 600; }
.views input { margin-right: 8px; accent-color: var(--accent); } .views label { display: inline-block; margin: 0 0 10px; cursor: pointer; }
.views .panel-full { display: none; } .views input:checked ~ .panel-main { display: none; } .views input:checked ~ .panel-full { display: block; }
svg.spine { display: block; width: 100%; height: auto; padding: 8px; background: var(--surface); margin: 0 auto; }
.spine .edge { fill: none; stroke-width: 1.5; } .spine .edge-user { stroke: var(--muted); } .spine .edge-system { stroke: var(--muted); } .spine .edge-route { stroke: var(--muted); stroke-dasharray: 2 4; } .spine .edge-fail { stroke: var(--bad); stroke-dasharray: 7 4; } .spine .edge-unused { stroke: var(--line); }
.spine .edge-main { stroke: var(--accent); stroke-width: 3.5; stroke-dasharray: none; }
.spine .arrow { fill: var(--muted); } .spine .arrow-fail { fill: var(--bad); } .spine .arrow-main { fill: var(--accent); }
.spine .elabel rect { fill: var(--surface); stroke: var(--line); } .spine .elabel text { fill: var(--ink); font: 13px system-ui, sans-serif; } .spine .elabel-fail text { fill: var(--bad); } .spine .elabel-main rect { stroke: var(--accent); }
.spine .node rect { fill: var(--surface); stroke: var(--ink); stroke-width: 1.6; } .spine .node-server rect { fill: var(--server); stroke: var(--muted); stroke-dasharray: 5 3; }
.spine .node-subflow rect { stroke: var(--accent); } .spine .node-subflow rect.inner { fill: none; stroke-width: 1; }
.spine .on-main rect { stroke-width: 3; } .spine .node-server.on-main rect { stroke: var(--accent); }
.spine .exit-never rect { stroke: var(--bad); stroke-width: 3; } .spine .exit-error rect { stroke-dasharray: 6 3; } .spine .exit-left rect { stroke-dasharray: 2 3; stroke: var(--muted); } .spine .exit-success rect { stroke: var(--ok); stroke-width: 3; }
.spine .node.by-human rect { fill: var(--surface); stroke: var(--ink); stroke-dasharray: none; }
.spine .node.by-ai rect { fill: var(--ai-bg); stroke: var(--ai); stroke-dasharray: none; }
.spine .node.by-agent rect { fill: var(--agent-bg); stroke: var(--agent); stroke-dasharray: none; } .spine .node.by-agent rect.inner { fill: none; stroke-width: 1; }
.spine .node.by-ci rect { fill: var(--ci-bg); stroke: var(--ci); stroke-dasharray: none; }
.spine .node.goal rect { fill: var(--ok-bg); stroke: var(--ok); stroke-width: 3.5; stroke-dasharray: none; }
.spine .node.on-main rect { stroke-width: 3.5; }
.spine .nmore { fill: var(--muted); font: 600 11px system-ui, sans-serif; }
.legend .sw { display: inline-flex; align-items: center; gap: 6px; } .legend svg { flex: none; }
.legend .sw-node rect { stroke-width: 1.6; }
.legend .sw-human rect { fill: var(--surface); stroke: var(--ink); } .legend .sw-ai rect { fill: var(--ai-bg); stroke: var(--ai); } .legend .sw-agent rect { fill: var(--agent-bg); stroke: var(--agent); } .legend .sw-agent rect.inner { fill: none; stroke-width: 1; }
.legend .sw-ci rect { fill: var(--ci-bg); stroke: var(--ci); } .legend .sw-goal rect { fill: var(--ok-bg); stroke: var(--ok); stroke-width: 3; } .legend .sw-server rect { fill: var(--server); stroke: var(--muted); stroke-dasharray: 4 2; }
.legend .sw-screen rect { fill: var(--surface); stroke: var(--ink); } .legend .sw-subflow rect { fill: var(--surface); stroke: var(--accent); }
.legend .sw-node.sw-fail rect { fill: var(--surface); stroke: var(--bad); stroke-width: 3; }
.legend .sw-line path { fill: none; stroke-width: 3; } .legend .sw-main path { stroke: var(--accent); stroke-width: 3.5; } .legend .sw-fail path { stroke: var(--bad); stroke-dasharray: 6 3; } .legend .sw-route path { stroke: var(--muted); stroke-dasharray: 2 3; }
.covers { display: grid; gap: 10px; } .covers .cv { border: 1px solid var(--line); border-radius: 10px; background: var(--surface); padding: 10px 14px; } .covers ul { margin: 4px 0 0; padding-left: 20px; }
.spine .node-unreachable rect { stroke: var(--bad); fill: none; }
.spine .nname { fill: var(--ink); font: 600 14.5px system-ui, sans-serif; } .spine .nline { fill: var(--muted); font: 11.5px ui-monospace, Menlo, Consolas, monospace; } .spine .start { fill: var(--ink); }
.spine a:focus-visible .node rect, .spine a:hover .node rect { stroke: var(--focus); stroke-width: 3; }
.legend { display: flex; flex-wrap: wrap; gap: 4px 18px; margin: 10px 0 0; font-size: 14px; color: var(--muted); }
.screens { display: grid; gap: 18px; }
.screen { border: 1px solid var(--line); border-radius: 10px; background: var(--surface); padding: 14px 16px 16px; scroll-margin-top: 16px; }
.screen:target { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-bg); }
.screen header { display: flex; align-items: center; gap: 10px; } .screen > p { margin: 4px 0 10px; }
.wf { border: 1px dashed var(--line); border-radius: 8px; padding: 14px; display: grid; gap: 10px; background: var(--bg); max-width: 560px; }
.wf-item { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; padding: 2px; border-radius: 4px; }
.wf-cond { outline: 1.5px dashed var(--muted); outline-offset: 2px; } .wf-focus { outline: 2.5px solid var(--focus); outline-offset: 2px; }
.wf-h { margin: 0; font-size: 17px; } .wf-t { margin: 0; color: var(--muted); }
.wf-row { display: flex; flex-wrap: wrap; gap: 10px; }
.wf-btn, .wf-link { font: inherit; border-radius: 6px; padding: 6px 14px; border: 1px solid var(--ink); background: var(--surface); color: var(--ink); }
.wf-confirm { background: var(--ink); color: var(--bg); } .wf-link { border: 0; background: none; color: var(--accent); text-decoration: underline; padding: 0; }
.wf button:disabled { opacity: .45; }
.wf-list { margin: 0; padding-left: 20px; display: grid; gap: 2px; }
.wf-status { color: var(--muted); font-style: italic; } .wf-alert { border: 1.5px solid var(--bad); border-radius: 6px; padding: 6px 10px; }
.wf-field { display: grid; gap: 4px; } .wf-input { font: inherit; border: 1px solid var(--muted); border-radius: 6px; padding: 6px 10px; background: var(--surface); color: var(--ink); }
.wf-unmapped { border: 1.5px solid var(--bad); padding: 6px 10px; }
.chip { font: 11px ui-monospace, Menlo, Consolas, monospace; color: var(--muted); border: 1px solid var(--line); border-radius: 4px; padding: 0 5px; white-space: nowrap; }
details { margin-top: 12px; } summary { cursor: pointer; color: var(--muted); }
details.ax-wrap { margin-top: 10px; } pre.ax { margin: 8px 0 0; padding: 10px 12px; border: 1px solid var(--line); border-radius: 8px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--bg); line-height: 1.55; }
dl.guards { margin: 10px 0 0; display: grid; grid-template-columns: max-content 1fr; gap: 3px 14px; } dl.guards dt { font-weight: 600; } dl.guards dd { margin: 0; }
ul.checks { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; } ul.checks li { padding-left: 26px; position: relative; } ul.checks li::before { content: "✓"; position: absolute; left: 4px; color: var(--ok); font-weight: 700; } ul.checks li.fail::before { content: "✗"; color: var(--bad); }
footer { margin-top: 56px; color: var(--muted); font-size: 13px; }
`;

export interface PageInput {
  g: Graph;
  m: Model;
  ex: Exploration;
  assertions: AssertionResult[];
  finals: FinalPath[];
  main: Story | null;
  spineMain: string;
  spineFull: string;
  screens: string;
  kit?: Kit | null;
  handOvers?: HandOver[];
}

function routingSection(g: Graph, ex: Exploration): string {
  const out: string[] = [];
  for (const [name, st] of Object.entries(g.flow.states)) {
    if (st.always === undefined) continue;
    const counts = ex.branchCounts.get(name) ?? [];
    const total = ex.atState.get(name) ?? 0;
    const rules = st.always;
    out.push(`<h3 style="margin:18px 0 8px">${esc(titleOf(g.flow, name))}</h3>`);
    out.push('<div class="scroll"><table><thead><tr><th>#</th><th>If</th><th>Then</th></tr></thead><tbody>');
    rules.forEach((a, i) => {
      const cond = a.say ?? (a.guard === undefined ? 'nothing above applies' : a.guard);
      out.push(`<tr><td>${i + 1}</td><td>${esc(cond)}</td><td>${esc(titleOf(g.flow, a.target))}</td></tr>`);
    });
    out.push('</tbody></table></div>');
    out.push('<p class="muted" style="margin:8px 0 0">The first matching rule wins.</p>');
    out.push('<details><summary>Exact conditions</summary><div class="scroll" style="margin-top:8px"><table><thead><tr><th>#</th><th>Condition</th><th>Goes to</th><th>Situations</th></tr></thead><tbody>');
    rules.forEach((a, i) => {
      out.push(`<tr><td>${i + 1}</td><td>${a.guard === undefined ? '<span class="muted">otherwise</span>' : code(a.guard)}</td><td>${code(a.target)}</td><td class="mono">${counts[i] ?? 0} of ${total}</td></tr>`);
    });
    out.push('</tbody></table></div>');
    const used = new Set<string>();
    for (const a of rules) if (a.guard !== undefined) identifiers(parseExpr(a.guard)).forEach((n) => used.add(n));
    const named = Object.entries(g.flow.guards).filter(([n]) => used.has(n));
    if (named.length > 0) {
      out.push('<dl class="guards">');
      for (const [n, expr] of named) out.push(`<dt>${code(n)}</dt><dd>${code(expr)}</dd>`);
      out.push('</dl>');
    }
    out.push('</details>');
  }
  return out.join('\n');
}

function transitionsTable(g: Graph, ex: Exploration): string {
  const rows: string[] = [];
  for (const [name, st] of Object.entries(g.flow.states)) {
    const add = (event: string, guard: string | undefined, target: string, effect: string[] | undefined, taken: boolean): void => {
      rows.push(
        `<tr><td>${code(name)}</td><td>${code(event)}</td><td>${guard === undefined ? '' : code(guard)}</td><td>${code(target)}</td><td>${(effect ?? []).map(code).join(' ')}</td><td class="${taken ? '' : 'bad'}">${taken ? 'yes' : 'never'}</td></tr>`,
      );
    };
    (st.always ?? []).forEach((a, i) => add(`rule ${i + 1}`, a.guard, a.target, a.effect, ex.taken.has(`${name}|always|${i}`)));
    for (const [event, spec] of Object.entries(st.on ?? {})) {
      alternatives(spec).forEach((a, i) => add(event, a.guard, a.target, a.effect, ex.taken.has(`${name}|${event}|${i}`)));
    }
  }
  return `<div class="scroll" style="margin-top:8px"><table><thead><tr><th>From</th><th>Event</th><th>Guard</th><th>To</th><th>Effects</th><th>Taken</th></tr></thead><tbody>${rows.join('\n')}</tbody></table></div>`;
}

function legend(g: Graph, ex: Exploration, hasMain: boolean): string {
  const states = Object.entries(g.flow.states);
  const edges = collectEdges(g, ex);
  const node = (cls: string, rx: number, inner = false): string =>
    `<svg width="34" height="20" viewBox="0 0 34 20" aria-hidden="true" class="sw-node sw-${cls}"><rect x="1.5" y="1.5" width="31" height="17" rx="${rx}"/>${inner ? '<rect class="inner" x="4.5" y="4.5" width="25" height="11" rx="2"/>' : ''}</svg>`;
  const line = (cls: string): string => `<svg width="34" height="20" viewBox="0 0 34 20" aria-hidden="true" class="sw-line sw-${cls}"><path d="M2 10 L32 10"/></svg>`;
  const items: string[] = [];
  const add = (sw: string, text: string): void => {
    items.push(`<span class="sw">${sw}<span>${esc(text)}</span></span>`);
  };
  if (hasMain) add(line('main'), 'blue line: the main story');
  if (edges.some((e) => e.kind === 'fail')) add(line('fail'), 'red dashed line: a failure path (shown with “Show failure paths”)');
  if (edges.some((e) => e.kind === 'route')) add(line('route'), 'dotted line: decided by a rule, nobody acts');
  const by = new Set(states.map(([, s]) => s.by).filter((b) => b !== undefined));
  if (by.has('human')) add(node('human', 4), 'a person does it (editor, review, approval)');
  if (by.has('ai')) add(node('ai', 9), 'AI does it');
  if (by.has('agent')) add(node('agent', 4, true), 'an agent does it, on the real app or for a user');
  if (by.has('ci')) add(node('ci', 0), 'CI does it, automatically');
  if (states.some(([, s]) => s.goal === true)) add(node('goal', 4), 'green: the goal the flow keeps returning to');
  const kinds = new Set(states.filter(([, s]) => s.by === undefined && s.goal !== true).map(([n]) => kindOf(g, n)));
  if (kinds.has('server')) add(node('server', 4), 'dashed grey box: the server decides, nothing is shown');
  if (kinds.has('screen')) add(node('screen', 4), 'solid box: the user sees a screen (click it)');
  if (kinds.has('subflow')) add(node('subflow', 4, true), 'double border: another flow');
  if (states.some(([, s]) => s.type === 'final' && s.redirect === 'success')) add(node('goal', 9), 'green exit: success');
  if (states.some(([, s]) => s.type === 'final' && s.redirect === 'never')) add(node('fail', 9), 'red exit: no redirect');
  if (states.some(([, s]) => (s.includes ?? []).length > 0)) add('<b class="muted">+n</b>', 'the step covers n more things (listed below)');
  return `<p class="legend">${items.join('')}</p>`;
}

export function renderPage(p: PageInput): string {
  const { g, m, ex, assertions, finals, main } = p;
  const kit = p.kit ?? null;
  const holds = assertions.filter((a) => a.violation === undefined).length;
  const heading = g.flow.title ?? humanize(g.flow.id);
  const out: string[] = [];
  out.push('<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">');
  out.push(`<title>${esc(heading)}</title><style>${CSS}${kit === null ? '' : kitCss(kit, 'page')}</style></head><body><main>`);
  out.push(`<p class="eyebrow">${g.flow.status === undefined ? '' : `<span class="tag">${esc(g.flow.status)}</span> `}<span class="mono">${esc(g.flow.id)}</span></p>`);
  out.push(`<h1>${esc(heading)}</h1>`);
  if (g.flow.summary !== undefined) out.push(`<p class="lede">${esc(g.flow.summary.trim())}</p>`);
  const sc = g.flow.scope;
  if (sc !== undefined) {
    out.push('<div class="story" style="border-left-color:var(--muted)"><p class="label">What this view shows</p>');
    if (sc.shows !== undefined) out.push(`<p style="margin:0">${esc(sc.shows)}.</p>`);
    if (sc.world === 'open') out.push('<p class="muted" style="margin:6px 0 0">Open world: what is not drawn here is not ruled out. Only the guarantees below are binding.</p>');
    if ((sc.outOfView ?? []).length > 0) out.push(`<p style="margin:8px 0 0"><b>Left out of this view:</b></p><ul style="margin:4px 0 0">${(sc.outOfView ?? []).map((o) => `<li>${esc(o)}</li>`).join('')}</ul>`);
    out.push('</div>');
  }

  if (main !== null) {
    out.push('<section class="story"><h2>The main story</h2>');
    out.push(`<p class="given"><b>When:</b> ${esc(main.given)}.</p><ol>`);
    for (const s of main.steps) out.push(`<li>${esc(s)}</li>`);
    out.push(`</ol><p class="result">Result: ${esc(main.result)}.</p></section>`);
  }

  out.push('<h2>The flow</h2>');
  out.push('<div class="views"><input type="checkbox" id="show-all"><label for="show-all">Show failure paths and “the user closes the page” <span class="muted">(wider: scrolls sideways)</span></label>');
  out.push(`<div class="scroll panel-main">${p.spineMain}</div><div class="scroll panel-full">${p.spineFull}</div></div>`);
  out.push(legend(g, ex, main !== null));
  const covers = Object.entries(g.flow.states).filter(([, s]) => (s.includes ?? []).length > 0);
  if (covers.length > 0) {
    out.push('<h2>What each step covers</h2><div class="covers">');
    for (const [n, s] of covers) out.push(`<div class="cv"><b>${esc(titleOf(g.flow, n))}</b><ul>${(s.includes ?? []).map((i) => `<li>${esc(i)}</li>`).join('')}</ul></div>`);
    out.push('</div>');
  }

  if (p.screens.trim() !== '') {
  out.push('<h2>What the user sees</h2>');
  if (kit === null) out.push('<p class="muted" style="margin-top:-4px">Wireframes are generated from the UI graph with fixed components, so they show structure, wording and state, not the final look.</p>');
  else out.push(`<p class="muted" style="margin-top:-4px">Screens are drawn from the UI graph with the components of ${esc(kit.title)} (kit <span class="mono">${esc(kit.id)}</span>): the look is the kit's, the wording and state are the graph's. A component the kit does not cover is drawn with the fixed one.</p>`);
  out.push(`<div class="screens">${p.screens}</div>`);
  }

  const answers = Object.entries(g.flow.states).filter(([, st]) => st.response !== undefined);
  if (answers.length > 0) {
    out.push('<h2>What each step answers</h2>');
    out.push('<div class="scroll"><table><thead><tr><th>Step</th><th>The outside world sees</th></tr></thead><tbody>');
    for (const [name, st] of answers) out.push(`<tr><td>${esc(titleOf(g.flow, name))}<br><span class="mono muted">${esc(name)}</span></td><td>${esc(responseLine(st.response ?? {}))}</td></tr>`);
    out.push('</tbody></table></div>');
  }

  const routing = routingSection(g, ex);
  if (routing !== '') {
    out.push('<h2>How it decides what happens next</h2>');
    out.push(routing);
  }

  if (finals.length === 0) out.push('<h2>Every way it can end</h2><p class="muted">It never ends: every path returns to a state the flow keeps.</p>');
  else {
  out.push('<h2>Every way it can end</h2>');
  out.push('<div class="scroll"><table><thead><tr><th>It ends with</th><th>When</th><th>What happens</th></tr></thead><tbody>');
  for (const f of finals) {
    const story = narrate(g, m, stepsTo(m, ex, f.index));
    out.push(
      `<tr><td><b>${esc(story.result)}</b><br><span class="muted">${esc(exitLine(g.flow.states[f.state]))}</span></td><td>${esc(story.given)}</td><td><ol>${story.steps.map((s) => `<li>${esc(s)}</li>`).join('')}</ol></td></tr>`,
    );
  }
  out.push('</tbody></table></div>');
  out.push('<p class="muted" style="margin:8px 0 0">Each row is also a generated test: set the starting situation, send the events, expect the ending.</p>');
  }

  const hands = p.handOvers ?? [];
  if (hands.length > 0) {
    out.push('<h2>The flows it starts</h2>');
    out.push('<p class="muted" style="margin-top:-4px">This flow starts them and does not wait. Each begins with what this flow knows at that moment.</p>');
    out.push('<div class="scroll"><table><thead><tr><th>From</th><th>It starts</th><th>Which begins with</th><th>Checked</th></tr></thead><tbody>');
    for (const h of hands) {
      const way = h.event === null ? 'a rule' : (pressLabel(g, h.from, h.event) ?? humanize(h.event).toLowerCase());
      const given = Object.entries(h.given).map(([k, v]) => `${code(k)} = ${code(String(v))}`).join('<br>') || '<span class="muted">its own beginning</span>';
      const checked = !h.found ? '<span class="muted">not written yet</span>' : h.problems.length === 0 ? 'fits' : `<span class="bad">${h.problems.map(esc).join('<br>')}</span>`;
      out.push(`<tr><td>${esc(titleOf(g.flow, h.from))}<br><span class="muted">${esc(way)}</span></td><td class="mono">${esc(h.flow)}</td><td>${given}</td><td>${checked}</td></tr>`);
    }
    out.push('</tbody></table></div>');
  }

  out.push('<h2>What is guaranteed</h2>');
  out.push(`<p>These hold in all ${ex.configs.length} situations the model can reach, from ${ex.starts} different starting situations${holds === assertions.length ? '' : ` — <span class="bad">${assertions.length - holds} do not</span>`}.</p>`);
  out.push('<ul class="checks">');
  for (const a of assertions) {
    const v = a.violation;
    const text = esc(a.say ?? humanize(a.id));
    out.push(
      v === undefined
        ? `<li>${text}</li>`
        : `<li class="fail"><b>${text}</b> <span class="bad">— violated</span> in <code>${esc(v.state)}</code> after <span class="mono">${esc(v.events.join(' > ') || '(start)')}</span></li>`,
    );
  }
  out.push('</ul>');
  out.push('<details><summary>Exact rules</summary><div class="scroll" style="margin-top:8px"><table><thead><tr><th>Id</th><th>Never</th></tr></thead><tbody>');
  for (const a of assertions) out.push(`<tr><td>${code(a.id)}</td><td>${code(a.never)}</td></tr>`);
  out.push('</tbody></table></div></details>');

  if (g.flow.decisions !== undefined) {
    out.push('<h2>Still to decide</h2>');
    out.push('<div class="scroll"><table><thead><tr><th>Id</th><th>Question</th><th>Recommendation</th><th>Affects</th></tr></thead><tbody>');
    for (const [id, d] of Object.entries(g.flow.decisions)) {
      out.push(`<tr><td>${esc(id)}</td><td>${esc(d.question)}</td><td>${esc(d.recommend)}</td><td>${d.affects.map((s) => esc(titleOf(g.flow, s))).join(', ')}</td></tr>`);
    }
    out.push('</tbody></table></div>');
  }

  out.push('<h2>Reference</h2>');
  out.push(`<details><summary>All transitions (${Object.values(g.flow.states).reduce((n, s) => n + (s.always?.length ?? 0) + Object.values(s.on ?? {}).reduce((k, spec) => k + alternatives(spec).length, 0), 0)})</summary>${transitionsTable(g, ex)}</details>`);
  const from = g.ui.absent === true ? 'flow.yaml (it has no screens)' : `flow.yaml, ui.yaml${kit === null ? ' and the catalog' : `, the catalog and the kit ${esc(kit.id)}`}`;
  out.push(`<footer>delurio graph-viz · generated from ${from} · no AI, no network: the same input gives the same bytes.</footer>`);
  out.push('</main></body></html>');
  return out.join('\n') + '\n';
}
