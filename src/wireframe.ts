import { titleOf } from './graph.ts';
import type { Graph, UiNode } from './graph.ts';
import { drawWith, mark } from './kit.ts';
import type { Fill, Kit } from './kit.ts';

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

type Warn = (message: string) => void;

function fill(g: Graph, key: string, warn: Warn): string {
  const raw = g.catalog[key];
  if (raw === undefined) {
    warn(`catalog key ${key} is missing`);
    return `⟨missing ${key}⟩`;
  }
  return raw.replace(/\{(\w+)\}/g, (_m, p: string) => {
    const sample = g.ui.samples[p];
    if (sample === undefined) {
      warn(`no sample value for {${p}} in ${key}`);
      return `⟨${p}⟩`;
    }
    return sample;
  });
}

function texts(g: Graph, n: UiNode, warn: Warn): string[] {
  const t = n.text;
  if (t === undefined) return [];
  if (t.key !== undefined) return [fill(g, t.key, warn)];
  if (t.oneOf !== undefined) return t.oneOf.map((k) => fill(g, k, warn));
  if (t.literal !== undefined) return [t.literal];
  return [];
}

function accessibleName(g: Graph, n: UiNode, warn: Warn): string | undefined {
  const nm = n.name;
  if (nm?.key !== undefined) return fill(g, nm.key, warn);
  if (nm?.literal !== undefined) return nm.literal;
  if (nm?.labelledBy !== undefined) {
    const ref = g.ui.nodes[nm.labelledBy];
    return ref === undefined ? undefined : texts(g, ref, warn).join(' ');
  }
  if (n.role === 'button' || n.role === 'heading' || n.role === 'status' || n.role === 'alert') return texts(g, n, warn)[0];
  return undefined;
}

function shownIn(n: UiNode, state: string): boolean {
  return (n.in ?? []).includes(state) && !(n.unless ?? []).includes(state);
}

interface Item {
  id: string;
  node: UiNode;
  disabled: boolean;
  focus: boolean;
}

function chips(it: Item): string {
  const n = it.node;
  const parts = [n.role ?? n.type];
  if (n.emits !== undefined) parts.push(`emits ${n.emits}`);
  if (it.disabled) parts.push('disabled');
  if (n.when !== undefined) parts.push(`when ${n.when}`);
  if (it.focus) parts.push('first focus');
  return `<span class="chip">${esc(parts.join(' · '))}</span>`;
}

function renderWireframe(g: Graph, state: string, items: Item[], warn: Warn, clean = false, kit: Kit | null = null): string {
  // clean: no annotation chips and no dashed or focus outlines, for the tutorial stage
  const item = (it: Item, inner: string): string => {
    const cls = clean ? 'wf-item' : ['wf-item', it.node.when !== undefined ? 'wf-cond' : '', it.focus ? 'wf-focus' : ''].filter((c) => c !== '').join(' ');
    return `<div class="${cls}" data-node="${esc(it.id)}">${inner}${clean ? '' : chips(it)}</div>`;
  };
  const out: string[] = [];
  let inList = false;
  let inRow = false;
  let kitList: { it: Item; items: string[] } | null = null; // a List drawn with the kit waits for its items
  const closeList = (): void => {
    if (kitList !== null) out.push(drawKitList(kitList.it, kitList.items));
    else if (inList) out.push('</ul>');
    kitList = null;
    inList = false;
  };
  const fill = (it: Item, text: string): Fill => {
    const n = it.node;
    const name = accessibleName(g, n, warn);
    // the role the expected tree gives the node, which picks a template from the component's `roles`
    const role = n.role ?? (n.type === 'text' ? undefined : n.type);
    return { id: it.id, text, name: name ?? text, intent: n.intent, role, disabled: it.disabled, named: name !== undefined && name !== text, conditional: n.when !== undefined };
  };
  const drawKitList = (it: Item, drawn: string[]): string => {
    const f = { ...fill(it, texts(g, it.node, warn)[0] ?? ''), children: drawn.map((d) => `\n${d}`).join('') + '\n' };
    return mark(drawWith(kit as Kit, 'List', f, warn) ?? '', it.id, '');
  };
  const closeRow = (): void => {
    if (inRow) out.push('</div>');
    inRow = false;
  };
  for (const it of items) {
    const n = it.node;
    const t = texts(g, n, warn);
    const text = esc(t[0] ?? '');
    if (n.component !== 'ListItem') closeList();
    if (n.component !== 'Button') closeRow();
    if (kit !== null && n.component !== undefined && n.component !== 'Page') {
      if (kit.components[n.component] !== undefined) {
        if (n.component === 'List') {
          kitList = { it, items: [] };
          inList = true;
        } else if (n.component === 'ListItem') {
          if (!inList) warn(`${it.id}: ListItem outside a List in ${state}`);
          for (const line of t) (kitList?.items ?? out).push(mark(drawWith(kit, 'ListItem', fill(it, line), warn) ?? '', it.id, clean ? '' : chips(it)));
        } else {
          if (n.component === 'Button' && !inRow) out.push('<div class="wf-row">');
          if (n.component === 'Button') inRow = true;
          out.push(item(it, drawWith(kit, n.component, fill(it, t[0] ?? ''), warn) ?? ''));
        }
        continue;
      }
      warn(`kit ${kit.id} has no template for ${n.component} (node ${it.id}); drawn with the fixed component`);
    }
    switch (n.component) {
      case 'Page':
        break;
      case 'Heading':
        out.push(item(it, `<h4 class="wf-h">${text}</h4>`));
        break;
      case 'Text':
        out.push(item(it, `<p class="wf-t">${text}</p>`));
        break;
      case 'Button':
        if (!inRow) out.push('<div class="wf-row">');
        inRow = true;
        out.push(item(it, `<button type="button" class="wf-btn wf-${esc(n.intent ?? 'secondary')}"${it.disabled ? ' disabled' : ''}>${text}</button>`));
        break;
      case 'LinkButton':
        out.push(item(it, `<button type="button" class="wf-link"${it.disabled ? ' disabled' : ''}>${text}</button>`));
        break;
      case 'TextField':
        // the visible text is the label; the input is named by it, as a <label> around it does
        out.push(item(it, `<label class="wf-field"><span>${text}</span><input type="text" class="wf-input" tabindex="-1"${it.disabled ? ' disabled' : ''}></label>`));
        break;
      case 'List':
        out.push(`<ul class="wf-list" data-node="${esc(it.id)}">`);
        inList = true;
        break;
      case 'ListItem':
        if (!inList) warn(`${it.id}: ListItem outside a List in ${state}`);
        for (const line of t) (kitList?.items ?? out).push(`<li data-node="${esc(it.id)}">${esc(line)}${clean ? '' : chips(it)}</li>`);
        break;
      case 'Status':
        out.push(item(it, `<div class="wf-status" role="status">${text}</div>`));
        break;
      case 'Alert':
        out.push(item(it, `<div class="wf-alert" role="alert">${text}</div>`));
        break;
      default:
        warn(`${it.id}: component ${n.component ?? '(none)'} has no wireframe renderer`);
        out.push(item(it, `<div class="wf-unmapped">${esc(n.component ?? n.type)} ${esc(it.id)}</div>`));
    }
  }
  closeList();
  closeRow();
  return out.join('\n');
}

function renderTree(g: Graph, items: Item[], warn: Warn): string {
  const lines: string[] = [];
  for (const it of items) {
    const n = it.node;
    const depth = n.component === 'Page' ? 0 : n.component === 'ListItem' ? 2 : 1;
    const flags = [it.disabled ? 'disabled' : '', n.when !== undefined ? `when ${n.when}` : '', it.focus ? 'first focus' : '', n.emits !== undefined ? `→ ${n.emits}` : '']
      .filter((f) => f !== '')
      .join(', ');
    const suffix = flags === '' ? '' : `  [${flags}]`;
    const pad = '  '.repeat(depth);
    if (n.component === 'ListItem') {
      for (const line of texts(g, n, warn)) lines.push(`${pad}listitem "${line}"${suffix}`);
      continue;
    }
    const role = n.role ?? (n.type === 'text' ? 'text' : n.type);
    const name = role === 'text' ? texts(g, n, warn)[0] : accessibleName(g, n, warn);
    lines.push(`${pad}${role}${name === undefined ? '' : ` "${name}"`}${suffix}`);
  }
  return lines.join('\n');
}

export function renderScreens(g: Graph, warn: Warn, kit: Kit | null = null): string {
  const out: string[] = [];
  for (const [state, st] of Object.entries(g.flow.states)) {
    const shown = Object.entries(g.ui.nodes).filter(([, n]) => shownIn(n, state));
    if (shown.length === 0) continue;
    const focusTargets = new Set(shown.map(([, n]) => n.initialFocus).filter((f): f is string => f !== undefined));
    const items: Item[] = shown.map(([id, node]) => ({
      id,
      node,
      disabled: (node.disabledIn ?? []).includes(state),
      focus: focusTargets.has(id),
    }));
    out.push(`<article class="screen" id="screen-${esc(state)}"><header><h3>${esc(titleOf(g.flow, state))}</h3><span class="tag">${st.type === 'final' ? 'ends here, page shown' : 'screen'}</span><span class="mono muted">${esc(state)}</span></header>`);
    if (st.description !== undefined) out.push(`<p class="muted">${esc(st.description)}</p>`);
    out.push(`<div class="wf${kit === null ? '' : ' kit'}" role="group" aria-label="Wireframe of ${esc(titleOf(g.flow, state))}">`);
    out.push(renderWireframe(g, state, items, warn, false, kit));
    out.push('</div>');
    out.push(`<details class="ax-wrap"><summary>Expected accessibility tree</summary><pre class="ax">${esc(renderTree(g, items, warn))}</pre></details>`);
    out.push('</article>');
  }
  return out.join('\n');
}

// One state's screen for the tutorial stage. Nodes that depend on a flag (`when`) appear only when named in `reveal`.
export function renderStageScreen(g: Graph, state: string, reveal: string[], warn: Warn, kit: Kit | null = null): string {
  const shown = Object.entries(g.ui.nodes).filter(([id, n]) => shownIn(n, state) && (n.when === undefined || reveal.includes(id)));
  const items: Item[] = shown.map(([id, node]) => ({ id, node, disabled: (node.disabledIn ?? []).includes(state), focus: false }));
  return `<div class="wf${kit === null ? '' : ' kit'}" role="group" aria-label="Screen: ${esc(titleOf(g.flow, state))}">${renderWireframe(g, state, items, warn, true, kit)}</div>`;
}

// What the graph expects a screen reader to find for one drawn node: `index` counts the elements drawn for the node in
// its screen (a ListItem draws one per text).
export interface Expectation {
  state: string;
  id: string;
  index: number;
  role: string;
  name: string | undefined;
  disabled: boolean;
}

// For `kit check`: every screen with all its nodes (flags revealed, no annotations), and what each node must expose.
export function renderCheckScreens(g: Graph, kit: Kit | null, warn: Warn): { html: string; expected: Expectation[] } {
  const html: string[] = [];
  const expected: Expectation[] = [];
  for (const state of Object.keys(g.flow.states)) {
    const shown = Object.entries(g.ui.nodes).filter(([, n]) => shownIn(n, state));
    if (shown.length === 0) continue;
    const items: Item[] = shown.map(([id, node]) => ({ id, node, disabled: (node.disabledIn ?? []).includes(state), focus: false }));
    html.push(`<section class="screen" data-state="${esc(state)}"><div class="wf${kit === null ? '' : ' kit'}">${renderWireframe(g, state, items, warn, true, kit)}</div></section>`);
    for (const it of items) {
      const n = it.node;
      if (n.component === 'Page') continue; // the screen itself: nothing is drawn for it
      if (n.component === 'ListItem') {
        texts(g, n, warn).forEach((line, index) => expected.push({ state, id: it.id, index, role: n.role ?? 'listitem', name: line, disabled: it.disabled }));
        continue;
      }
      const role = n.role ?? (n.type === 'text' ? 'text' : n.type);
      expected.push({ state, id: it.id, index: 0, role, name: role === 'text' ? texts(g, n, warn)[0] : accessibleName(g, n, warn), disabled: it.disabled });
    }
  }
  return { html: html.join('\n'), expected };
}

// the visible words of a node, for narration: a node with several texts gives several entries
export function nodeTexts(g: Graph, id: string, warn: Warn): string[] {
  const n = g.ui.nodes[id];
  return n === undefined ? [] : texts(g, n, warn);
}
