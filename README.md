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

## What a revision changes, in words

```sh
npx @delurio/cli diff examples/qauth-authorize --base origin/main
```

This prints Markdown for a pull request. It covers what the flow does differently (found by running both revisions) and what changed in the graph.

## Licence

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). The name "delurio" is not licensed with the code.
