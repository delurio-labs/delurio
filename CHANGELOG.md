# Changelog

While the version is 0.x, a release that can judge the same graphs differently (a new check, a stricter schema) bumps the minor; a fix that judges nothing differently bumps the patch.

## Unreleased

- SPEC.md §7: a tutorial step may say how it comes out where an effect leaves it to chance (`havoc`): `then { <variable>: <value> }`. The step takes the case in which those variables have those values; without `then` it takes the model's first case. A tutorial can now go through an answer from outside (a host that lists the person's repositories) that the graph leaves open. The checker does not read tutorials, so nothing is judged differently.

## 0.7.0 (2026-11-12)

- An action may be a server function (SPEC.md §5): `serverFn: GET` or `serverFn: POST` in place of a route, for a call that the product's own pages make to its server through their framework, such as TanStack Start's `createServerFn` or React's server functions, at an address the framework gives it. It is not part of the product's HTTP API: it is no OpenAPI operation, its input is its `body`, and it has no `query` and no `operationId`. `GET` reads and changes nothing; `POST` may write. An address that the browser itself opens, such as a link in an e-mail or a sign-in callback, keeps its route even when a server function answers it. Before, such an action could only be written with a route, as an operation that others may call, or with none, as a step inside the service; the `schema` rule refused `serverFn`.
- New rule of `check`, which fails `--strict`: `server-fn-http`, a server function that also has a `route`, a `query` or an `operationId`. It is given at the `serverFn`, with a note at the other key, in whichever files of a split domain each is given. A method other than `GET` or `POST` is an error of the `schema` rule.
- The `changes` rule (SPEC.md §4) reads a server function's method as it reads a route's: a `POST` writes and a `GET` does not, even when it returns an entity. Graphs with no `serverFn` are judged as before.
- `page`: a flow's page lists what each of its steps calls, under "What each step calls": the action, and how it is called (an HTTP operation and its route, a server function and its method, or a step inside the service). It reads the flow's domain where the `changes` rule finds it; a flow whose domain is not found gets no list. The other parts of the examples' pages are unchanged.
- `examples/notes/restore`: a flow with a screen whose two actions are server functions, one `GET` and one `POST`, in the split notes domain.

## 0.6.1 (2026-11-11)

- `page`: in the flow's diagram, every state's group carries `data-state`, and every edge and its label `data-from` and `data-to`. One edge is drawn per pair of states, so a transition is found by its two states. A tool on top of the CLI can now find a state or a transition in the drawing, to move to it or link to it. The pages of the examples differ from 0.6.0's only by these attributes. Nothing is judged differently.
- `@delurio/cli/spine` exports `renderSpine`, which draws that diagram, with the states to draw as the story.
- SPEC.md §7: a tutorial may be told on the flow's diagram instead of its screens (`stage: diagram`), for a flow with no screens. `show` and `end` walk the flow to their state along its normal way, and every other event is given with `after`; an ending needs no card. The checker does not read tutorials, so nothing is judged differently.

## 0.6.0 (2026-11-10)

- A graph file with an anchor (`&name`), an alias (`*name`), a tag (`!name`) or a key that is a list or a map cannot be loaded (SPEC.md §1): `check` gives `load` and `schema` errors at its line, "unsupported YAML", and exits 1; `page`, `diff` and the other commands stop the same way. The yaml library resolves these in time far beyond a file's size. On a 40 KB file, maps used as keys kept it busy for more than 30 seconds, and aliases of an anchor that holds only aliases of an empty list for more than two minutes. Its alias limit does not catch these cases. Every reader of a graph file now looks for them first, in one pass that resolves nothing: such a file is refused in under a second. A value is written where it is used, and a key is a string. Of the graphs of delurio, Medaris and QAuth, only QAuth's has them: seven flows use anchors for a `response` they repeat.
- `@delurio/cli/yaml` exports that reader, `readYaml`, and `unsupported`, which names the first such place. A server that reads graphs it was sent should read them with it.
- New rule of `check`, which fails `--strict`: `source-missing`, a state of a captured flow (`status: captured`) that names no code (SPEC.md §9). An exit may leave it out, as the transition into it names the code. Flows that are not captured are judged as before.

## 0.5.3 (2026-11-06)

- `page`: the `flow.mmd` it writes now reads whole in Mermaid. Mermaid ends a statement at a `;`, even inside a transition's label, so a label with one lost its rest: those words were drawn as stray states, or, when a `-` or `{` came after the `;`, the diagram did not render at all ("Parse error on line …", QAuth's sign-in in an editor's preview). A `;` in a label is now written as a comma. A transition never taken and with no label of its own now carries "(never taken)" as its label: before, those two words were drawn as two more states. Of the 106 flow diagrams of QAuth, Medaris and delurio, 86 were drawn wrong (36 did not render, 50 had stray states) and none is now, in Mermaid 10.9 and 12.3; their 5 navigation diagrams were right before and are unchanged. `index.html` draws its own diagram and was not affected. Nothing is judged differently.

## 0.5.2 (2026-11-03)

- `page`: the flow's diagram opens fitted to a box as wide as the window, and moves like a design tool's canvas: drag it to move (the hand), Ctrl or ⌘ with the wheel, or a pinch, to zoom at the pointer, and the wheel alone to move it until its edge, after which the page scrolls on. Buttons and keys: − and + (zoom), the zoom level (click: 100%, Shift+0), Fit (Shift+1), Full window (F; Esc leaves it); arrows move it. A click that does not drag still opens a step's screen. Before, a large flow was shrunk into the page's column (QAuth's consent flow to 7%) or scrolled sideways at full size. Without scripts the page keeps the old diagram. Nothing is judged differently.

## 0.5.1 (2026-11-03)

- `check`, `diff` and `page` explore a large flow about four times faster and in a third of the memory, and judge every graph exactly as before: the same situations in the same order, the same shortest paths and the same findings. On QAuth's consent flow (5.4 million situations from 580,608 starts) `check` took 122 s and 2.9 GB, and now takes 34 s and 1 GB. A situation is kept as a number instead of a string; guards, assertions and effects are read once instead of at every situation; and an assertion is not tested in the states where the state alone rules it out.

## 0.5.0 (2026-11-01)

- A domain may be split across the files of its directory (SPEC.md §5): every `*.yaml` there that says `kind: domain`, read as one, `domain.yaml` first and then the others by name. Maps merge key by key at any depth, `policies` and `invariants` join, and any other value is given in one file only. `check` (the `schema` and `changes` rules), `diff` and `page` read the merged domain; before, a domain file not named `domain.yaml` was read by none of them.
- New rules of `check`, which fail `--strict`: `domain-twice`, a value or an invariant given in two files of a domain, at the second one's line with a note at the first; `domain-place`, a file whose id is not its directory's domain, or a domain whose files sit in two directories.
- `diff` heads the domain's changes "The API it calls (its domain)", and names every file of it it compared.
- An entity may carry `examples` (SPEC.md §5): records of it, each an object of its fields, as JSON Schema's own keyword. The `schema` rule refused it before.

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
