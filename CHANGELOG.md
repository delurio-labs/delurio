# Changelog

While the version is 0.x, a release that can judge the same graphs differently (a new check, a stricter schema) bumps the minor; a fix that judges nothing differently bumps the patch.

## Unreleased

- A domain may be split across the files of its directory (SPEC.md §5): every `*.yaml` there that says `kind: domain`, read as one, `domain.yaml` first and then the others by name. Maps merge key by key at any depth, `policies` and `invariants` join, and any other value is given in one file only. `check` (the `schema` and `changes` rules), `diff` and `page` read the merged domain; before, a domain file not named `domain.yaml` was read by none of them.
- New rules of `check`, which fail `--strict`: `domain-twice`, a value or an invariant given in two files of a domain, at the second one's line with a note at the first; `domain-place`, a file whose id is not its directory's domain, or a domain whose files sit in two directories.
- `diff` heads the domain's changes "The API it calls (its domain)", and names every file of it it compared.

## 0.4.0 (2026-11-01)

- An exit (`type: final`) may be named in snake_case, optionally followed by its cause: `signed_out`, `invalid_token.dpop_bound` (SPEC.md §1, Text). The `schema` rule accepts these names on exits only; every other state stays camelCase. `flow.mmd` writes such an id as `invalid_token__dpop_bound`, since Mermaid ids take no dot.

## 0.3.0 (2026-10-25)

- The modules a tool on top of delurio needs are importable, with their types: `@delurio/cli/graph`, `/model`, `/story`, `/wireframe`, `/kit`, `/edges`, `/html`, `/nav` and `/expr`. While the version is 0.x their API may change in any minor release; pin an exact version.
- SPEC.md §7: a video's AI disclosure is its first card, which says that the whole video is made with AI, in place of a label people see throughout.

## 0.2.0 (2026-10-25)

- JSON Schemas (2020-12) for `flow`, `ui`, `nav` (and `nav.extracted.yaml`), `domain` and `catalog` files, shipped in `schemas/`, for editors: completion, hover help from SPEC.md, and errors such as a typo in a key or a title cut short by a comma inside `{ }`.
- `page --watch` writes the pages again on every change under the folder; `page --serve [--port <n>]` also serves them on 127.0.0.1 and reloads the open page after each write, or says why a graph could not be loaded.
- `check` validates every graph file against its kind's schema: each place a file does not follow it is an error of the new rule `schema`, at its line and column, which fails `--strict` (exit 2) and works with `--baseline`.

## 0.1.1 (2026-10-25)

- `page` on a folder writes the page of every graph under it and `index.html` listing them, in one process.
- A folder that holds no graph gets one line instead of a stack trace; counts read in the singular when there is one.
- The README runs every command as `npx @delurio/cli`.

## 0.1.0 (2026-10-25)

- `check`: every graph under a folder, as GNU error lines or SARIF 2.1.0, with `--baseline`; exit 0, 1 or 2.
- `page`: a graph's page in words, with its guarantees, screens and diagram.
- `diff`: what a revision changes in a graph, in words, for a pull request.
