import { CSS } from './html.ts';
import { pathTo } from './nav.ts';
import type { Entry, Finding, FrameId, Goalcheck, MenuRow, NavGraph, Page, PersonaView, RepoCheck, Standing } from './nav.ts';

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const code = (s: string): string => `<code>${esc(s)}</code>`;

const NAV_CSS = `
.persons { display: grid; gap: 12px; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); }
.person { border: 1px solid var(--line); border-radius: 10px; background: var(--surface); padding: 12px 14px; }
.person h3 { margin-bottom: 4px; } .person p { margin: 4px 0 0; } .person .stat { margin-top: 8px; font-size: 14px; }
.finding { margin: 12px 0; padding: 12px 16px; border: 1px solid var(--line); border-left: 5px solid var(--muted); border-radius: 10px; background: var(--surface); }
.finding.problem { border-left-color: var(--bad); } .finding p { margin: 0; } .finding ul { margin: 8px 0 0; padding-left: 20px; } .finding li { margin: 3px 0; }
.finding .tag { margin-right: 6px; }
table.matrix th, table.matrix td { padding: 6px 8px; } table.matrix th.persona { min-width: 92px; text-transform: none; letter-spacing: 0; font-size: 12px; }
table.matrix td.cell { text-align: center; white-space: nowrap; } td.cell .why { display: block; font-size: 11px; color: var(--muted); }
.c-ok { color: var(--ok); font-weight: 600; } .c-refused { color: var(--bad); font-weight: 600; } .c-placeholder { color: var(--muted); font-weight: 600; } .c-missing { color: var(--bad); font-weight: 600; } .c-hidden, .c-none { color: var(--line); }
tr.grp th { background: var(--accent-bg); color: var(--ink); text-transform: none; letter-spacing: 0; font-size: 13px; }
.path { display: block; font-size: 12px; color: var(--muted); max-width: 190px; white-space: normal; text-align: left; }
td.reach { vertical-align: top; } td.reach b { font-size: 15px; }
.legend-row { display: flex; flex-wrap: wrap; gap: 4px 18px; margin: 10px 0 0; font-size: 14px; color: var(--muted); }
ul.sitemap { list-style: none; margin: 0; padding: 0; } ul.sitemap li { padding: 6px 0; border-bottom: 1px solid var(--line); } ul.sitemap li:last-child { border-bottom: 0; }
ul.sitemap .route { margin-left: 8px; color: var(--muted); }
.checklist li.fail::before { content: "✗"; color: var(--bad); }
`;

const STANDING_WORD: Record<Standing, string> = { ok: 'opens', placeholder: 'not built', missing: 'not found', refused: 'refuses', unreachable: 'no way' };
const STANDING_MARK: Record<Standing, string> = { ok: '✓', placeholder: '◌', missing: '✗', refused: '⛔', unreachable: '–' };

function pathWords(entries: Entry[]): string {
  return entries
    .filter((e) => e.label !== '')
    .map((e) => e.label)
    .join(' → ');
}

function landingStats(g: NavGraph, v: PersonaView, rows: MenuRow[], index: number): string {
  if (v.landing === null) return '';
  const frame = g.pages[v.landing].frame;
  if (!g.menuFrames.includes(frame)) return '';
  const shown = rows.filter((r) => r.frame === frame && r.cells[index].shown);
  if (shown.length === 0) return `The ${(g.frames[frame].menu ?? frame).toLowerCase()} has no entries.`;
  const by = (s: Standing): number => shown.filter((r) => r.cells[index].standing === s).length;
  const parts = [`${by('ok')} open`];
  if (by('refused') > 0) parts.push(`${by('refused')} refuse them`);
  if (by('placeholder') > 0) parts.push(`${by('placeholder')} are not built`);
  if (by('missing') > 0) parts.push(`${by('missing')} lead to a page that does not exist`);
  return `The ${(g.frames[frame].menu ?? frame).toLowerCase()} has ${shown.length} ${shown.length === 1 ? 'entry' : 'entries'}: ${parts.join(', ')}.`;
}

function findingHtml(f: Finding): string {
  const detail = f.detail.length === 0 ? '' : `<ul>${f.detail.map((d) => `<li>${esc(d)}</li>`).join('')}</ul>`;
  return `<section class="finding ${f.level}"><p><span class="tag ${f.level === 'problem' ? 'bad' : ''}">${f.level}</span>${esc(f.say)}</p>${detail}</section>`;
}

function matrix(g: NavGraph, views: PersonaView[], rows: MenuRow[]): string {
  const out: string[] = ['<div class="scroll"><table class="matrix"><thead><tr><th>Menu entry</th>'];
  for (const v of views) out.push(`<th class="persona">${esc(v.persona.title)}</th>`);
  out.push('</tr></thead><tbody>');
  for (const frame of g.menuFrames) {
    out.push(`<tr class="grp"><th colspan="${views.length + 1}">${esc(g.frames[frame].menu ?? frame)}</th></tr>`);
    for (const r of rows.filter((x) => x.frame === frame)) {
      const target = g.pages[r.entry.to];
      out.push(`<tr><td>${esc(r.entry.label)}<span class="path">${esc(target.title === r.entry.label ? target.route : `${target.title} · ${target.route}`)}</span></td>`);
      r.cells.forEach((c, i) => {
        if (!r.frameKnown[i]) out.push('<td class="cell c-none" title="this kind of user never has this menu"></td>');
        else if (!c.shown || c.standing === null) out.push('<td class="cell c-hidden" title="not in this person’s menu">–</td>');
        else out.push(`<td class="cell c-${c.standing}" title="${STANDING_WORD[c.standing]}">${STANDING_MARK[c.standing]}<span class="why">${STANDING_WORD[c.standing]}</span></td>`);
      });
      out.push('</tr>');
    }
  }
  out.push('</tbody></table></div>');
  out.push(`<p class="legend-row"><span><b class="c-ok">✓ opens</b> nothing in the application’s own code refuses it (the server can still say no)</span><span><b class="c-refused">⛔ refuses</b> the entry is drawn, the page says no</span><span><b class="c-placeholder">◌ not built</b> the page says it is not ready</span>${Object.values(g.pages).some((pg) => pg.kind === 'missing') ? '<span><b class="c-missing">✗ not found</b> there is no such page</span>' : ''}<span>– not in this person’s menu</span><span>blank: this kind of user never has this menu</span></p>`);
  return out.join('');
}

function reachTable(g: NavGraph, views: PersonaView[]): string {
  const out: string[] = ['<div class="scroll"><table class="matrix"><thead><tr><th>Page</th>'];
  for (const v of views) out.push(`<th class="persona">${esc(v.persona.title)}</th>`);
  out.push('</tr></thead><tbody>');
  for (const frame of Object.keys(g.frames)) {
    const pages = Object.values(g.pages).filter((p) => p.frame === frame && p.id !== 'start');
    if (pages.length === 0) continue;
    out.push(`<tr class="grp"><th colspan="${views.length + 1}">${esc(g.frames[frame].title)}</th></tr>`);
    for (const p of pages) {
      out.push(`<tr><td>${esc(p.title)}${p.kind === 'placeholder' ? ' <span class="tag">not built</span>' : p.kind === 'missing' ? ' <span class="tag">does not exist</span>' : ''}<span class="path">${esc(p.route)}</span></td>`);
      for (const v of views) {
        const st = v.standing[p.id];
        if (st === 'unreachable') {
          out.push('<td class="reach c-hidden">–</td>');
          continue;
        }
        const r = v.reach.get(p.id);
        const steps = r === undefined ? '' : pathWords(pathTo(v.reach, p.id));
        const mark = st === 'refused' ? ' <span class="c-refused">⛔</span>' : st === 'placeholder' ? ' <span class="c-placeholder">◌</span>' : st === 'missing' ? ' <span class="c-missing">✗</span>' : '';
        out.push(`<td class="reach"><b>${r?.clicks ?? 0}</b>${mark}${r?.usesData === true ? ' †' : ''}<span class="path">${esc(steps === '' ? 'automatic' : steps)}</span></td>`);
      }
      out.push('</tr>');
    }
  }
  out.push('</tbody></table></div>');
  out.push('<p class="legend-row"><span>number: fewest clicks from the moment the person signs in and “/” has sent them on</span><span>⛔ the page refuses them</span><span>◌ the page is not built</span><span>† needs an item in a list (a row to pick, a card to open)</span><span>–: no way there</span></p>');
  return out.join('');
}

function goalsTable(g: NavGraph, goals: Goalcheck[]): string {
  if (goals.length === 0) return '';
  const out: string[] = ['<h2>Things people come to do</h2><div class="scroll"><table><thead><tr><th>Goal</th><th>Who</th><th>Clicks</th><th>Result</th></tr></thead><tbody>'];
  for (const c of goals) {
    const ok = c.outcome === 'ok';
    out.push(`<tr><td>${esc(c.goal.say)}<span class="path">${esc(g.pages[c.goal.page].route)}</span></td><td>${esc(c.persona.title)}</td><td>${c.clicks === null ? '–' : `${c.clicks} of ${c.goal.within}`}</td><td class="${ok ? 'ok' : 'bad'}">${ok ? 'ok' : esc(c.outcome)}</td></tr>`);
  }
  out.push('</tbody></table></div>');
  return out.join('');
}

function siteMap(g: NavGraph): string {
  const out: string[] = [];
  for (const frame of Object.keys(g.frames)) {
    const pages = Object.values(g.pages).filter((p) => p.frame === frame);
    if (pages.length === 0) continue;
    out.push(`<h3 style="margin:16px 0 4px">${esc(g.frames[frame].title)}</h3><ul class="sitemap">`);
    for (const p of pages) {
      const tags: string[] = [];
      if (p.kind !== 'screen') tags.push(p.kind === 'placeholder' ? 'not built' : p.kind === 'missing' ? 'does not exist' : p.kind);
      if (p.opensFor !== null) tags.push(`opens only when ${p.opensFor}`);
      const notes = [p.opensFor !== null && p.refusal !== null ? `Otherwise ${p.refusal}.` : null, p.evidence].filter((n): n is string => n !== null);
      out.push(`<li>${esc(p.title)}<span class="route">${code(p.route)}</span> ${tags.map((t) => `<span class="tag">${esc(t)}</span>`).join(' ')}${notes.map((n) => `<br><span class="muted">${esc(n)}</span>`).join('')}</li>`);
    }
    out.push('</ul>');
  }
  return out.join('');
}

function referenceTable(g: NavGraph): string {
  const out: string[] = ['<div class="scroll" style="margin-top:8px"><table><thead><tr><th>From</th><th>How</th><th>Label</th><th>To</th><th>Only when</th></tr></thead><tbody>'];
  g.specs.forEach((s) => {
    const from = (Array.isArray(s.from) ? s.from : [s.from]).join(', ');
    const target = g.pages[s.to];
    const extra = [s.data === true ? 'needs an item in a list' : '', s.back === true ? 'goes back' : ''].filter((x) => x !== '').join('; ');
    out.push(`<tr><td>${code(from)}</td><td>${esc(s.how)}</td><td>${esc(s.label === undefined ? '' : (g.catalog[s.label] ?? s.label))}</td><td>${esc(target.title)} ${code(target.route)}</td><td>${s.when === undefined ? '' : code(s.when)}${extra === '' ? '' : ` <span class="muted">${esc(extra)}</span>`}</td></tr>`);
  });
  out.push('</tbody></table></div>');
  return out.join('');
}

function guardsReference(g: NavGraph): string {
  const out: string[] = ['<dl class="guards">'];
  for (const [n, expr] of Object.entries(g.guards)) out.push(`<dt>${code(n)}</dt><dd>${code(expr)}</dd>`);
  out.push('</dl>');
  return out.join('');
}

export interface NavPageInput {
  g: NavGraph;
  views: PersonaView[];
  rows: MenuRow[];
  goals: Goalcheck[];
  found: Finding[];
  repo: { checks: RepoCheck[]; head: string | null } | null;
  warnings: string[];
}

export function renderNavPage(p: NavPageInput): string {
  const { g, views, rows, goals, found, repo } = p;
  const out: string[] = [];
  const problems = found.filter((f) => f.level === 'problem');
  out.push('<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">');
  out.push(`<title>${esc(g.title)} navigation</title><style>${CSS}${NAV_CSS}</style></head><body><main>`);
  out.push(`<p class="eyebrow"><span class="tag">${esc(g.status)}</span> <span class="mono">${esc(g.id)}</span> · navigation graph</p>`);
  out.push(`<h1>${esc(g.title)}: where people go</h1>`);
  if (g.summary !== '') out.push(`<p class="lede">${esc(g.summary)}</p>`);
  out.push(
    `<p class="muted">${Object.keys(g.pages).length} pages · ${g.specs.length} ways to move · ${views.length} kinds of user${g.source.commit === undefined ? '' : ` · read from ${esc(String(g.source.repo ?? 'the app'))} at ${esc(g.source.commit)}`} · wording: ${esc(g.locale)}</p>`,
  );

  out.push('<h2>What stands out</h2>');
  out.push(
    problems.length === 0 ? '<p>Nothing blocks anyone.</p>' : `<p>${problems.length} problem${problems.length === 1 ? '' : 's'} and ${found.length - problems.length} note${found.length - problems.length === 1 ? '' : 's'}. Problems send a person somewhere that does not work; notes are about words.</p>`,
  );
  for (const f of found) out.push(findingHtml(f));

  out.push('<h2>Who uses it</h2><div class="persons">');
  views.forEach((v, i) => {
    const landing = v.landing === null ? null : g.pages[v.landing];
    out.push(`<div class="person"><h3>${esc(v.persona.title)}</h3><p class="muted">${esc(v.persona.say)}</p>`);
    out.push(`<p class="stat">Lands on <b>${esc(landing === null ? '–' : landing.title)}</b>${landing === null ? '' : ` ${code(landing.route)}`}${landing?.kind === 'placeholder' ? ' <span class="tag">not built</span>' : ''}.</p>`);
    const line = landingStats(g, v, rows, i);
    if (line !== '') out.push(`<p class="stat">${esc(line)}</p>`);
    out.push('</div>');
  });
  out.push('</div>');

  out.push('<h2>What the menu shows each kind of user</h2>');
  const menus = g.menuFrames.map((f) => (g.frames[f].menu ?? f).toLowerCase());
  out.push(`<p class="muted" style="margin-top:-4px">One row per menu entry.${menus.length > 1 ? ` The ${menus.join(' and the ')} are different menus.` : ''}</p>`);
  out.push(matrix(g, views, rows));

  out.push('<h2>How to get to each page</h2>');
  out.push(reachTable(g, views));

  out.push(goalsTable(g, goals));

  out.push('<h2>The pages</h2>');
  out.push(siteMap(g));

  if (repo !== null) {
    const bad = repo.checks.filter((c) => !c.ok);
    out.push('<h2>Checked against the app</h2>');
    out.push(
      `<p>${repo.checks.length - bad.length} of ${repo.checks.length} checks hold${repo.head === null ? '' : ` in the checkout at ${esc(repo.head)}`}${repo.head !== null && g.source.commit !== undefined && !repo.head.startsWith(g.source.commit) && !g.source.commit.startsWith(repo.head) ? ` <span class="bad">(the graph was read at ${esc(g.source.commit)})</span>` : ''}. The checks read the route files, the menu registry, the permission table and the translation file; they do not run the app.</p>`,
    );
    const line = (c: RepoCheck): string => `<li class="${c.ok ? '' : 'fail'}">${esc(c.what)}${c.detail === '' ? '' : ` <span class="muted">— ${esc(c.detail)}</span>`}</li>`;
    if (bad.length > 0) out.push(`<ul class="checks checklist">${bad.map(line).join('')}</ul>`);
    out.push(`<details><summary>${repo.checks.length - bad.length} checks that hold</summary><ul class="checks checklist" style="margin-top:8px">${repo.checks.filter((c) => c.ok).map(line).join('')}</ul></details>`);
  }

  out.push('<h2>Not modeled</h2><ul>');
  for (const n of g.notModeled) out.push(`<li>${esc(n)}</li>`);
  out.push('</ul>');

  out.push('<h2>Reference</h2>');
  out.push(`<details><summary>Every way to move (${g.specs.length})</summary>${referenceTable(g)}</details>`);
  out.push(`<details><summary>Named rules</summary>${guardsReference(g)}</details>`);
  out.push('<footer>delurio graph-viz · generated from nav.yaml and the catalog · no AI, no network: the same input gives the same bytes.</footer>');
  out.push('</main></body></html>');
  return out.join('\n') + '\n';
}

const clean = (s: string): string => s.replace(/"/g, "'").replace(/[<>|]/g, ' ').replace(/\s+/g, ' ').trim();

// Mermaid flowchart for PRs and READMEs: one menu hub per frame; links, buttons and landing rules as arrows.
export function renderNavMermaid(g: NavGraph): string {
  const lines: string[] = ['---', `title: ${clean(g.title)} navigation`, '---', 'flowchart LR'];
  const hubOf = (p: Page): string | null => (g.menuFrames.includes(p.frame) ? `hub_${p.frame}` : null);
  for (const frame of Object.keys(g.frames)) {
    const pages = Object.values(g.pages).filter((p) => p.frame === frame);
    if (pages.length === 0) continue;
    lines.push(`  subgraph F_${frame}["${clean(g.frames[frame].title.replace(/ \(.*$/, ''))}"]`);
    if (g.menuFrames.includes(frame)) lines.push(`    hub_${frame}(("menu"))`);
    for (const p of pages) lines.push(`    ${p.id}["${clean(p.title)}<br/>${clean(p.route)}"]`);
    lines.push('  end');
  }
  const seen = new Set<string>();
  const edge = (from: string, to: string, label: string, dotted: boolean): void => {
    const key = `${from}|${to}|${label}|${dotted}`;
    if (seen.has(key)) return;
    seen.add(key);
    lines.push(`  ${from} ${dotted ? '-.->' : '-->'}${label === '' ? '' : `|"${clean(label)}"|`} ${to}`);
  };
  for (const e of g.entries) {
    const from = g.pages[e.from];
    const hub = hubOf(from);
    if (e.how === 'menu') {
      if (hub !== null) edge(hub, e.to, '', false);
      else if (g.frames[from.frame].shows !== undefined) continue; // drawn through the hubs
    } else if (e.how === 'picker') {
      if (hub !== null) edge(hub, e.to, e.label, true);
    } else if (e.how === 'user') {
      if (hub !== null) edge(hub, e.to, e.label, true);
    } else {
      edge(e.from, e.to, e.how === 'auto' ? 'automatic' : e.label, e.how === 'auto');
    }
  }
  lines.push('  classDef placeholder stroke-dasharray:4 3,fill:#f5f5f4,stroke:#78716c');
  lines.push('  classDef hub fill:#eef3ff,stroke:#1d4ed8');
  const placeholders = Object.values(g.pages).filter((p) => p.kind === 'placeholder').map((p) => p.id);
  if (placeholders.length > 0) lines.push(`  class ${placeholders.join(',')} placeholder`);
  const gone = Object.values(g.pages).filter((p) => p.kind === 'missing').map((p) => p.id);
  if (gone.length > 0) {
    lines.push('  classDef missing stroke-dasharray:2 3,fill:#fff5f5,stroke:#b91c1c');
    lines.push(`  class ${gone.join(',')} missing`);
  }
  if (g.menuFrames.length > 0) lines.push(`  class ${g.menuFrames.map((f) => `hub_${f}`).join(',')} hub`);
  return lines.join('\n') + '\n';
}
