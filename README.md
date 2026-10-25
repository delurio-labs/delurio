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

- **Findings:** one line each, in the GNU error format that compilers, editors and CI logs read:

  ```
  graphs/billing/billing.payment/flow.yaml:176:7: error: restricted-only-when-retries-ran-out is broken: … [assertion-violated]
  graphs/billing/billing.payment/flow.yaml:176:7: note: start: role = manager, paid = false
  graphs/billing/billing.payment/flow.yaml:176:7: note: step 1: RETRIES_RAN_OUT
  ```

- **SARIF:** `--format sarif --output delurio.sarif` writes SARIF 2.1.0 for code scanning. The same input gives the same bytes.
- **Baseline:** `--baseline <sarif>` fails only on errors that are new since an earlier run.

| Exit | Meaning |
|---|---|
| 0 | every graph loads, and under `--strict` nothing is wrong |
| 1 | a graph cannot be loaded, or is written for a newer format than this version reads |
| 2 | under `--strict`: a guarantee is broken, a catalog key or a sample is missing, the main story cannot be walked, a writing flow has no changes note… |

## A graph's page

```sh
npx @delurio/cli page examples/qauth-authorize --out viz/qauth-authorize
```

The page shows the flow in words: its main story, every way it ends, its guarantees and whether they hold, its screens as wireframes, and its diagram. `examples/qauth-authorize` is a small graph to try it on.

Given a folder of graphs instead, it writes the page of every graph under it, each at its own path, and `index.html` listing them all with what their check found:

```sh
npx @delurio/cli page graphs --out viz
```

## Write a graph in an editor

The package ships a JSON Schema (2020-12) for each kind of graph file in `schemas/`: `flow`, `ui`, `nav` (also `nav.extracted.yaml`), `domain` and `catalog`. With them an editor completes keys, explains each one from SPEC.md on hover, and marks a typo, a value of the wrong kind, or a title cut short by a comma inside `{ }`. In VS Code, with the YAML extension, put this in `.vscode/settings.json`:

```json
{
  "yaml.schemas": {
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/flow.schema.json": "**/flow.yaml",
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/ui.schema.json": "**/ui.yaml",
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/nav.schema.json": ["**/nav.yaml", "**/nav.extracted.yaml"],
    "https://unpkg.com/@delurio/cli@0.2.0/schemas/domain.schema.json": "**/domain.yaml"
  },
  "json.schemas": [
    { "fileMatch": ["**/catalog/*.json"], "url": "https://unpkg.com/@delurio/cli@0.2.0/schemas/catalog.schema.json" }
  ]
}
```

With the package installed, `./node_modules/@delurio/cli/schemas/<kind>.schema.json` works the same, offline. `check` reports the same as the editor, as errors of the rule `schema` that fail `--strict`, and finds what a schema cannot see, such as a state that does not exist or a catalog key that is missing.

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

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). The name "delurio" is not licensed with the code.
