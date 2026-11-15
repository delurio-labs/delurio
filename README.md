# delurio

The open checker and page of **delurio-graph/0**, a format for writing a product's flows, screens and API as one graph that people review in words, that is checked against the code, replayed on the real app and turned into tutorials. The format is [SPEC.md](SPEC.md).

This package is the part a project's own CI runs: it reads graph files and nothing else. There is no network, no account and no model.

## Install

Run it once, without installing:

```sh
npx @delurio/cli@0.1.1 check graphs --strict
```

or pin an exact version in a project, after which the command is `delurio`:

```sh
npm install --save-dev --save-exact @delurio/cli
npx delurio check graphs --strict
```

or the container image, pinned by its digest:

```sh
docker run --rm -v "$PWD:/work" -w /work ghcr.io/delurio-labs/delurio@sha256:<digest> check graphs --strict
```

Node 22.18 or later.

## Check every graph

```sh
npx @delurio/cli check graphs --strict
```

Several folders may be given (`check graphs examples`); a graph under two of them is checked once.

- **Findings:** one line each, in the GNU error format that compilers, editors and CI logs read:

  ```
  graphs/billing/billing.payment/flow.yaml:176:7: error: restricted-only-when-retries-ran-out is broken: … [assertion-violated]
  graphs/billing/billing.payment/flow.yaml:176:7: note: start: role = manager, paid = false
  graphs/billing/billing.payment/flow.yaml:176:7: note: step 1: RETRIES_RAN_OUT
  ```

- **SARIF:** `--format sarif --output delurio.sarif` writes SARIF 2.1.0 for code scanning. The same input gives the same bytes.
- **Baseline:** `--baseline <sarif>` fails only on errors that are new since an earlier run; a graph that cannot be loaded fails it, new or not.

| Exit | Meaning |
|---|---|
| 0 | every graph loads, and under `--strict` nothing is wrong |
| 1 | a graph cannot be loaded, or is written for a newer format than this version reads |
| 2 | under `--strict`: a guarantee is broken, a state cannot be reached or has no way out, a catalog key is missing from a locale or a sample is missing, the main story cannot be walked, a tutorial no longer matches its flow, a writing flow has no changes note, a domain names an entity or a field it does not have, a need's evidence is not there or a product's gaps and its needs disagree… |

## What a product must have: needs and gaps

A flow draws what the product does and may leave the rest out, so a feature that is missing is simply not drawn. Beside the flow, `needs.yaml` lists what must exist for its users, as a closed list: each need names its evidence (the code, a test, a scenario of a binding that walks it, the states and nodes it is drawn as, a setting a deployment gives), or says that it is `missing`, and why. Every missing need of a product is listed in one file at the root of its graphs, `needs.gaps.yaml` (SPEC.md §13). [`examples/notes/restore/needs.yaml`](examples/notes/restore/needs.yaml) has three needs that are met and one gap.

```
examples/notes/restore/needs.yaml:47:5: note: need says-when-it-is-erased is missing: archived notes are not erased yet; … [need-gap]
delurio check: 5 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0
```

`check --strict` fails where evidence names what is not there (a file, a line, a name, a scenario, a state, a node), where a missing need is not in the gaps, and where the gaps list a need that is met or that no flow has. A gap that is listed fails nothing: it is counted, and given as a note. So a product built in phases is written whole, the planned parts included, while its CI stays green, and the gaps only shrink: a need that is met must leave the list, and a new gap is a line added to it, which a review reads. Evidence paths are relative to the repository root, the folder `check` runs in; `--app <dir>` names another.

## A graph's page

```sh
npx @delurio/cli page examples/qauth-authorize --out viz/qauth-authorize
```

The page shows the flow in words: its main story, every way it ends, its guarantees and whether they hold, its screens as wireframes, what each step calls (an HTTP operation, a server function of the product's own pages, or a step inside the service), and its diagram, which opens fitted to the window and moves like a design tool's canvas (drag to move, Ctrl or ⌘ + scroll or a pinch to zoom, Shift+1 to fit). `examples/qauth-authorize` is a small graph to try it on.

Given a folder of graphs instead, it writes the page of every graph under it, each at its own path, and `index.html` listing them all with what their check found:

```sh
npx @delurio/cli page graphs --out viz
```

## Draw the screens with your product's look: a kit

A kit (SPEC.md §8) says how the components of `ui.yaml` look in one product, so that the page and the videos draw its screens as the product does rather than as plain wireframes. It is data, in a folder of its own: nothing in it runs or is fetched. [`examples/notes/kit`](examples/notes/kit) is a small kit to start from, which the screens of `examples/notes` are drawn with.

1. Copy `examples/notes/kit` beside your graphs (`graphs/kit/`) and give it your own `id` (lowercase letters, digits and dashes) and `title`.
2. Name it from each `ui.yaml` whose screens it draws, by its path from that graph's folder: `kit: ../kit`. `page --kit <path>` draws with another kit for one run, and `--kit none` draws the plain wireframes.
3. Put your design tokens in `tokens.json`, in the W3C Design Tokens format (2025.10): groups nest, a token has `$value`, `$type` is inherited from its group, and `"{color.accent}"` is an alias. Each token becomes a CSS custom property named by its path: `color.accent` is `--color-accent`, `font.size-body` is `--font-size-body`. `dark.json` gives the same names other values for a dark page; a name that `tokens.json` lacks is refused.
4. Write the CSS in `kit.css`, from the tokens. It is scoped to the drawn screen: `&` is the screen itself, a rule never reaches the rest of the page, `:root`, `html` and `body` are refused as they never match there, and only `@media`, `@supports` and `@container` may wrap rules. Nothing is fetched: `url()` takes only `data:` addresses.
5. Give each component its screens use a `template`, one HTML element or a few, filled per node:
   - slots: `{text}`, the node's visible text; `{name}`, its accessible name; `{id}`, the node's id; `{intent}`, the word `intents` gives its intent (`confirm`, `dismiss`, `secondary`, and `default` for a node with none); `{children}`, the items of a `List`;
   - flags, each a section that opens and closes in the same place: `{?disabled}..{/disabled}` where the node is disabled, `{!disabled}..{/disabled}` where it is not, `{?named}` where the graph gives an accessible name other than the visible text (so `aria-label="{name}"` goes inside it), and `{?conditional}` where the node shows only when a flag holds;
   - `roles`: a template of its own for a node whose ARIA role differs, such as a `Button` drawn as a link or a `ListItem` that is a checkbox or a radio;
   - a `List`'s and a `ListItem`'s template is one element, which the page marks with the node it draws.

   Templates take no `<script>`, `<style>`, `<iframe>`, `<form>` or event handler, and no address other than one inside the page (`#…`) or, for `src`, a `data:image`. Write the roles, names and disabled states the graph expects: a `<button>` for a button, a `<label>` around a field, `aria-label="{name}"` where it is named.

`Page` has no template: it is the screen itself, styled with `&`, and a dialog is a page node whose role is `dialog`, drawn as its screen. A component a screen uses that the kit has no template for, or an intent with no word in `intents`, is drawn with the fixed component and reported (`kit-uncovered`), which fails `--strict`. A kit that cannot be read, such as a template that is not well-formed or a token that refers to one no file defines, keeps every graph that names it from loading.

## Write a graph in an editor

The package ships a JSON Schema (2020-12) for each kind of graph file in `schemas/`: `flow`, `ui`, `nav` (also `nav.extracted.yaml`), `domain`, `catalog`, `needs` and `gaps` (`needs.gaps.yaml`). With them an editor completes keys, explains each one from SPEC.md on hover, and marks a typo, a value of the wrong kind, or a title cut short by a comma inside `{ }`. In VS Code, with the YAML extension, put this in `.vscode/settings.json`:

```json
{
  "yaml.schemas": {
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/flow.schema.json": "**/flow.yaml",
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/ui.schema.json": "**/ui.yaml",
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/nav.schema.json": ["**/nav.yaml", "**/nav.extracted.yaml"],
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/domain.schema.json": ["**/domain.yaml", "**/domain/*.yaml"],
    "https://unpkg.com/@delurio/cli@0.9.0/schemas/needs.schema.json": "**/needs.yaml",
    "https://unpkg.com/@delurio/cli@0.9.0/schemas/gaps.schema.json": "**/needs.gaps.yaml"
  },
  "json.schemas": [
    { "fileMatch": ["**/catalog/*.json"], "url": "https://unpkg.com/@delurio/cli@0.2.0/schemas/catalog.schema.json" }
  ]
}
```

A domain may be split across the files of its directory (SPEC.md §5); `**/domain/*.yaml` covers them in a directory named `domain`, and a file elsewhere names its schema on its first line: `# yaml-language-server: $schema=<the domain schema's URL>`. With the package installed, `./node_modules/@delurio/cli/schemas/<kind>.schema.json` works the same, offline. `check` reports the same as the editor, as errors of the rule `schema` that fail `--strict`, and finds what a schema cannot see, such as a state that does not exist or a catalog key that is missing.

Next to the editor, keep the pages open and current:

```sh
npx @delurio/cli page graphs --serve      # http://127.0.0.1:4600/, written again and reloaded on every save
```

`--watch` writes the pages again on every change without serving them. `--serve` listens on 127.0.0.1 only and serves the written pages and nothing else; when a save leaves a graph that cannot be loaded, the open page says why until the next good save.

## What a revision changes, in words

```sh
npx @delurio/cli diff examples/qauth-authorize --base origin/main
```

This prints Markdown for a pull request. It covers what the flow does differently (found by running both revisions) and what changed in the graph.

## As a library

A tool built on the format can use the same loader and checker the CLI uses:

```ts
import { load } from '@delurio/cli/graph';
import { explore, Model } from '@delurio/cli/model';

const g = load('graphs/billing/billing.payment');
const configurations = explore(new Model(g.flow)).configs.length;
```

The entry points are `graph`, `model`, `story`, `wireframe`, `kit`, `edges`, `html`, `nav`, `expr`, `yaml` and `needs`, each with its types. `yaml` reads a graph file's text as the CLI does (`readYaml`), refusing what SPEC.md §1 rules out before the text is resolved; a server that reads graphs it was sent should read them with it. `needs` reads a flow's needs (`readNeeds`) and gives the states and nodes that only missing needs are drawn as (`planned`), which a tool that walks the flow on the app leaves out. While the version is 0.x their API may change in any minor release: pin an exact version.

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). The name "delurio" is not licensed with the code.
