# Changelog

While the version is 0.x, a release that can judge the same graphs differently (a new check, a stricter schema) bumps the minor; a fix that judges nothing differently bumps the patch.

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
