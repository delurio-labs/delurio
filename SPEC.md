# delurio-graph/0

This is the format of a delurio graph, as the tools in this repository read it today. Everything in it is version 0: the format settles as delurio's own graphs and its first users (qauth among them) are written in it. A part that is still open says so and names the issue that tracks it. Nothing here is a proposal kept apart from the rest; what is written is what v0 is.

The tool behaviour behind each part (the page, the video, replay, anchors, kits, the revision diff) is described in `tools/graph-viz/README.md`. This file says what the files mean.

Where a standard already covers a part, the format uses it: JSON Schema and OpenAPI for the data and the API, WAI-ARIA for screens, ICU MessageFormat and BCP 47 for words, design tokens for kits, WebVTT for captions, SARIF for findings. What delurio adds is what ties them together: one graph whose flows, screens, pages and API are checked against each other, replayed on the real app and turned into tutorials. §12 lists every standard and what it is used for.

## 1. A graph and its files

A graph is a directory of YAML files. Every file starts with the same three keys:

```yaml
schema: delurio-graph/0
kind: flow            # flow | ui | domain | nav | binding | tutorial | kit
id: learning.enrollment
```

| Kind | File | What it says |
|---|---|---|
| `flow` | `flow.yaml` | the behaviour: states, events, transitions, a finite context, and guarantees the checker proves |
| `ui` | `ui.yaml` | what each state shows: the nodes of the screen, their roles, names, texts and the events they send |
| catalog | `catalog/<locale>.json` | the words of the UI, one file per locale |
| `domain` | `domain.yaml` | the data and the API: entities, actions, refusals, policies, invariants |
| `nav` | `nav.yaml` | the pages of an app, the ways between them, and who may open them |
| `binding` | `binding.yaml` | how a flow maps onto a running app, for replay and recording |
| `tutorial` | `tutorial.yaml` | an end-user video written against a flow |
| `kit` | `kit.yaml` | how the UI components look in one product |
| (generated) | `anchors.lock.yaml`, `viz/`, `video/` | written by the tools; never by hand |

A flow directory holds `flow.yaml`; a flow with screens also holds `ui.yaml` and the catalog that `ui.yaml` names. It may hold `domain.yaml`, `binding.yaml` and `tutorial.yaml`. A domain shared by several flows sits in its own directory and each flow names it by id (`domain: learning.api`). A navigation graph is a directory with `nav.yaml` and its catalog.

A flow with no screen (an HTTP endpoint, a webhook, an MCP tool, a CI job) has no `ui.yaml`: it loads and is checked without one, and its page draws no screens. What such a step answers the outside world is written on its state as `response` (§2).

### Common keys

| Key | Meaning |
|---|---|
| `title`, `summary` | the graph in plain words, for people |
| `status` | `proposed`: written before or apart from the code, nobody has approved it. `captured`: read from existing code (capture mode, every element carries `source:`). `approved`: the owner reviewed and accepted it |
| `source` | on any element: `<repo-relative path>:<line>` or a bare `<path>`, the code that makes the element true. See §9 |

### Text

The graph is written in one language, given by the project. Titles, `say` sentences, descriptions and assertion sentences are in that language. Every word a user of the product sees lives in a catalog, one file per locale; the graph refers to it by key. Identifiers (states, events, guards, effects, nodes, catalog keys) are English camelCase; events are UPPER_SNAKE.

## 2. Flow (`kind: flow`)

```yaml
schema: delurio-graph/0
kind: flow
id: qauth.authorize
status: proposed
domain: qauth.authorize     # the domain this flow's `invoke`s and refusals come from
ui: qauth.authorize         # the UI graph of its screens
title: Authorize and ask for consent
summary: >-
  ...
scope: { shows: ..., world: open, outOfView: [..] }
decisions: { D1: { question: ..., recommend: ..., affects: [state, ..] } }
context: { name: { values: [..], initial: any | <value>, say: { <value>: <words> } } }
guards: { name: "<expression>" }
effects: { name: { set: { var: value }, havoc: [var] } }
failures: { <invoking state>: { from: <action>, <class>: [<refusal code>, ..] } }
main: { given: { var: value }, events: [EVENT, ..] }
initial: <state>
states: { <state>: { .. } }
assertions: [ { id, never: "<expression>", say, source } ]
```

A flow is a statechart, and its meaning is a subset of SCXML's (W3C Recommendation, 2015). A transition in `on` is an SCXML transition with an event, `always` is a list of eventless transitions, a list of transitions is tried in document order, `invoke` and `type: final` mean what they mean in SCXML, a guard is a `cond` and an effect an `<assign>`; `DONE` and `FAIL.<class>` stand for SCXML's `done.invoke` and `error` events. A flow can therefore be written out as SCXML; that export is [#9](https://github.com/delurio-labs/delurio/issues/9).

### Context

The context is a set of variables, each with a finite list of `values`. `initial: any` makes the checker start from every value; a fixed `initial` starts from that one. `say` gives each value in plain words, for the sentence that describes a starting situation. Keep contexts small: a variable stands for the few cases that change behaviour, not for the data itself.

### Guards and effects

A guard is a named expression (§3). It may name context variables, other guards and `state`.

An effect changes the context when a transition is taken: `set` gives variables new values, `havoc` lets variables take any value of their domain (the checker branches into each), for a change made elsewhere that the flow cannot know in advance.

### States

| Key | Meaning |
|---|---|
| `title`, `description` | the state in words |
| `by` | who does the step: `human`, `ai`, `agent` or `ci`. A server step with no actor has none |
| `goal: true` | the state the flow is for, the one it keeps returning to (for flows that never end) |
| `includes: [..]` | what the step covers beyond its title |
| `invoke` | the step runs something: `{ action: <domain action> }`, `{ flow: <flow id> }` (another flow runs and this one waits for its outcome), or `{ external: <name> }` |
| `on` | event → transition (below) |
| `always` | an ordered list of transitions taken at once, with no event. The first whose guard holds is taken; the list must cover every context (the checker fails otherwise) |
| `type: final` | an exit; it has no transitions |
| `redirect`, `code`, `carries` | on an exit of a web protocol flow: `redirect: success \| error \| never`, the error `code`s, and what a success carries back |
| `leavesPage: true` | on an exit: the person left (closed the page, went elsewhere) |
| `response` | on a step with no screen: what the outside world sees, as data: `status`, `code`, `redirect`, `headers`, `body`, or the fields of a tool's answer. The page shows it beside the step, and a replay over HTTP will read it |
| `source` | the code that makes the state true |

A transition is a target state id, or `{ target, guard, effect: [..], say, starts, source }`, or a list of those: the first whose guard holds is taken. `say` is the transition in words; `starts` is under "Several flows" below.

### Events

- A state with `invoke` ends with `DONE` (success) or `FAIL.<class>`, where the class is a key under `failures` for that state; `failures.<state>.from` names the domain action, and each class lists the refusal codes of that action it stands for.
- `NAVIGATE` is the person leaving the page.
- An event caused by someone else (a course team, a webhook, another flow) is an event like any other; its name and `say` make clear who causes it.

### Main story, scope, decisions

- `main` is the story the page tells first and the path a tutorial and a replay take by default: the events, in order, from the `given` start. A plain list of events means no `given`. `--strict` fails when the events cannot be walked. Without `main`, the first success exit that passes a screen is the main story.
- `scope` says what the view shows (`shows`) and how to read what it does not draw. `world: open` means that what is not drawn is not ruled out; only `assertions` and `outOfView` are binding. `world: closed` means the flow is complete.
- `decisions` are the questions still open in this flow, each with a recommendation and the states it affects.

### Assertions and the checker

A configuration is a state together with a value for every context variable. The checker starts from every initial configuration and follows every transition whose guard holds, applying its effects, breadth first. An `always` state moves on at once. Every reachable configuration is visited once.

Each assertion is `never: <expression>`: it holds when no reachable configuration satisfies the expression. A violation is reported with the shortest event path from a starting situation. Under `--strict` a violated assertion exits 2.

The checker does not yet fail on unreachable states, transitions no situation takes, or states with no way out; the page already shows them. That is [#2](https://github.com/delurio-labs/delurio/issues/2). Paths that cover every transition are [#4](https://github.com/delurio-labs/delurio/issues/4).

### Several flows

Nested, parallel and history states are not part of v0, and a flow that uses them does not load. A process that runs beside another is its own flow:

- `invoke: { flow: <id> }`: this flow waits for the other one's outcome.
- `starts: { flow: <flow id>, given: { <variable>: <expression> } }` on a transition: this flow starts the other one and does not wait. `given` is what the started flow begins with: each value is an expression over this flow's context (§3), read where the transition is taken, after its effects; a constant is quoted (`from: "'repo'"`). The other flow's influence back on this flow is written as an event or an effect here. The checker finds the started flow by its id among the graphs beside this one (under the nearest folder called `graphs`) and checks that every situation in which the transition is taken gives each variable a value the started flow can begin with: one of its `values`, and its `initial` when that is fixed. A started flow that is not written yet is a note, not a failure, and is checked once it is. Decided on 22 Oct 2026; the form came with delurio.project.create.

## 3. Expressions

Guards and assertions are interpreted, never evaluated as code:

```
or  and  not  ==  !=  in  not in  ( )  [lists]  'strings'  numbers  true  false  null  names
```

A name is a context variable, a named guard or `state`. Every variable has a finite domain, so the checker can visit every reachable configuration. The same language is used by `nav.yaml` (over facts) and `ui.yaml` (`when`).

The language is delurio's own. CEL, JSONata or SCXML's ECMAScript data model can name values that have no finite list, and then the checker could no longer visit every configuration.

## 4. UI (`kind: ui`)

```yaml
schema: delurio-graph/0
kind: ui
id: qauth.authorize
flow: qauth.authorize
status: proposed
catalog: catalog/en.json
locales: [en]
kit: ../kit                 # optional: the kit the screens are drawn with
samples: { client: "Acme Notes" }   # values for catalog parameters, on the page and in videos
changes: { key: Changes.note }      # optional: what the flow changes and what follows, in its users' words
nodes: { <node>: { .. } }
edges: [ { labelledBy: [node, node] }, { triggers: [node, state] } ]
rules: [ { id, rule } ]
```

| Node key | Meaning |
|---|---|
| `type` | `page`, `text`, `action`, `field`, `list`, `status`, `alert` |
| `component` | a kit-neutral component: `Page`, `Heading`, `Text`, `Button`, `LinkButton`, `TextField`, `List`, `ListItem`, `Status`, `Alert`. A kit maps it to the product's own (§8). A `TextField`'s `text` is its label, and the field is named by it |
| `role` | the ARIA role the node exposes |
| `intent` | for actions: `confirm`, `dismiss`, `secondary` |
| `name` | the accessible name: `{ key, params }` from the catalog, `{ labelledBy: <node> }`, `{ literal }`, or `{ oneOf: [keys] }` |
| `text` | the visible text, same forms. Kept apart from `name` so that "the visible text is in the name" (WCAG 2.5.3) is checkable |
| `in`, `unless` | the flow states the node is shown in, and those it is not |
| `when` | a guard or context flag that must hold for the node to show |
| `disabledIn` | states where the node is shown but disabled |
| `emits` | the event the node sends when pressed |
| `initialFocus` | on a page: the node that has focus when it opens |
| `repeat` | the node is drawn once per item of this data (`request.scopes`) |
| `frame` | for a page: whether it may be framed (`never`) |
| `source` | the code that draws it |

`changes` is a catalog message that tells the flow's users, in their own words, what the flow changes and what follows from it: what is created, sent, charged or removed, who sees it, and whether it can be undone. Rehber shows it before it guides the flow and again at the step that makes the change, and it guides a flow whose steps write something only when the flow has one (rehber.install, rehber.guide; Taha, 24 Oct 2026: "readonly olmayan her akış için nelerin değişeceğini ve ne etkilerinin olacağını okuyabilecekleri bi not olsun"). The step that makes the change is found from the graph: the one that leads into a step invoking an action that writes. `graph-viz --strict` fails a flow whose steps invoke a writing action of its domain and whose ui has no `changes`, and a `changes` key missing from any locale's catalog (`tools/graph-viz/README.md`).

A `role` is a WAI-ARIA 1.2 role, and `name` is the accessible name that Accessible Name and Description Computation 1.2 gives the node; `kit check` reads both from the browser's accessibility tree.

A node is shown in a state when the state is in `in`, not in `unless`, and `when` holds. `edges` records relations the page and the checks use: a node labelled by another, an action that leads to a state. `rules` are written but no tool reads them yet; checking them, together with dialogs, focus return, confirmation before a destructive action and events handled where shown, is [#3](https://github.com/delurio-labs/delurio/issues/3).

### Catalog

A JSON object from key to an ICU MessageFormat message: the syntax of the catalogs apps already keep (ICU, FormatJS, next-intl), so a captured catalog is copied as it is. Today the tools fill simple arguments: `{param}` is filled from the node's `params`, which name sample or data values. A `plural` or `select` argument is read when a catalog first needs one. MessageFormat 2 (Unicode, stable since CLDR 47) is read when an app's own catalog is written in it.

One file per locale; `ui.yaml` names the one the page uses. A locale is a BCP 47 tag, in the file name (`en.json`, `tr.json`, `pt-BR.json`) and in `locales` and `locale`.

## 5. Domain (`kind: domain`)

```yaml
schema: delurio-graph/0
kind: domain
id: qauth.authorize
entities:
  Client:
    fields:
      id: { type: string }
      redirectUris: { type: array, items: { type: string, format: uri, pattern: "^https://" } }
      secret: { type: string, sensitivity: credential }
    lifecycle: [ "none -> active: ..." ]
  Consent:
    key: [userId, clientId]
    required: [userId, clientId, scopes]
    fields:
      userId: { type: string }
      clientId: { type: string, references: Client }
      scopes: { type: array, items: { type: string } }
policies:
  - { actor: end-user, needs: session, where: "...", read: [..], write: [Consent], do: [decide] }
actions:
  decide:
    route: POST /authorize/consent
    needs: session
    body: { type: object, required: [decision], properties: { decision: { enum: [allow, deny] } } }
    result: { returns: Consent }
    sideEffects: [audit:consent.decided]
    refusals: { CSRF_MISMATCH: { status: 403, redirect: never } }
invariants:
  - { id: redirect-uri-exact, rule: "...", spec: RFC 6749 §3.1.2.3 }
```

**The data.** An entity's `fields` give each field as a JSON Schema (2020-12), and `required` lists the fields that must be present: an entity is the JSON Schema `{ type: object, properties: <fields>, required: <required> }`. `null` is a type like any other: an optional time is `{ type: [string, "null"], format: date-time }`, and an enum that may be empty lists `null` among its values. A `body` or a `result` written out is a JSON Schema too. delurio adds these keywords, which a JSON Schema validator leaves alone:

| Keyword | On | Meaning |
|---|---|---|
| `key` | an entity | the fields that identify it; `[id]` when left out |
| `references` | a field | it holds the key of another entity |
| `unique` | a field | `true`, or `case-insensitive` |
| `sensitivity` | a field | `credential`, `personal` or `internal`: what may be shown, logged or kept |
| `lifecycle` | an entity | its states, as `from -> to: why` sentences |

A capture may carry more words of its own on a field (`owner`, `note`, `editableBy`, `matching`, `opaque`). They are kept and compared, and no tool reads them.

An action, a refusal and an invariant may carry `note`: the author's own words, which no tool reads. An invariant the code does not keep is still written, with `holds: false` and the `finding` that reports it: the finding's id in the graph's report, such as `MS-7`. A capture describes the code, so the invariant says what should hold and the finding says where it does not. Decided on 25 Oct 2026.

**The API.** An action with a `route` (`METHOD /path`) is one OpenAPI 3.1 operation: its name is the `operationId`, `needs` its security requirement, `query` (a JSON Schema object) its query parameters, `body` its request body, `result` its response (`returns` an entity, a `redirect`, a `status`, or a JSON Schema), and each refusal one more response. When a refusal has a body, it is RFC 9457 problem details with the refusal code as its `type`. So a domain can be written out as an OpenAPI document, and an app that already publishes one names it at the top (`openapi: <file>`) and each action's operation by `operationId`, instead of copying them. An action with no route is a step inside the service. `needs`, `policies`, `sideEffects`, `refusals.<code>.redirect` and `invariants` are delurio's own: no standard says who may do what to which data, or what must always hold.

This form was decided on 22 Oct 2026. Graphs captured before it may still give fields as `{ kind: text }`; they are converted when a tool first reads fields. The OpenAPI export and import are [#8](https://github.com/delurio-labs/delurio/issues/8).

No tool checks a domain yet: the revision diff compares it and anchors lock its `source:` keys. Loading it with the flow and checking policies, secrets and `invoke` names is [#5](https://github.com/delurio-labs/delurio/issues/5); generating it from Drizzle and Zod is [#6](https://github.com/delurio-labs/delurio/issues/6); a registry that every API operation and route is covered by a graph is [#7](https://github.com/delurio-labs/delurio/issues/7).

## 6. Navigation (`kind: nav`)

```yaml
kind: nav
schema: delurio-graph/0
id: shop-landing
title: Landing
status: captured
locale: tr
catalog: catalog/tr.json
summary: ...
frames: { site: { title: ..., menu: Footer } }
facts: { role: { values: [..], say: .. } }
sets: { staffRoles: [..] }
guards: { name: "<expression over facts>" }
personas: { visitor: { title, say, facts: { .. } } }
pages: { start: { route: /, title: <catalog key>, kind: screen, frame: site, source, opensFor, refusal } }
entries: [ { from: <page> | frame:<frame> | [..], to: <page>, how: menu, label: <catalog key>, when, clicks, data, back } ]
goals: [ { id, say, page, for: [persona], within: <clicks> } ]
notModeled: [ "..." ]
```

- A **frame** is the chrome around a page (a menu, a top bar). An entry `from: frame:<f>` is on every page of that frame.
- A **page** has a `kind`: `screen`, `placeholder`, `missing`, `redirect` or `external`. `opensFor` is the guard under which it opens; `refusal` says in words how it says no. A page called `start` is required.
- An **entry** is a way from page to page: `how` is `auto`, `menu`, `picker`, `user`, `link` or `button`; `when` is the guard under which it is offered; `data: true` means it needs an item to exist (a row to click).
- A **persona** is a fixed set of facts, so no search is needed: the tool works out each persona's view, menu and reachable pages, and checks every goal (the page is reached within the clicks).
- A `nav.yaml` may `extends` a generated `nav.extracted.yaml`: the generated pages and entries come first, and the hand-written file adds to them or overrides single fields.

## 7. Binding (`kind: binding`) and tutorial (`kind: tutorial`)

A **binding** ties a flow to a running app for replay and recording: `apps` (base addresses), `browser`, `data` (JSON or dotenv sources, secrets masked in reports), `signIn`, `accounts`, `scenarios` (each a `given` start, the account, the page to `open`, `setup` and `teardown`), `events` (the actions that cause an event no node on screen sends), `states` (a URL glob or a text a state must show) and `nodes` (a locator for a node that role and name cannot find, or `before` actions such as opening a menu, or `skip` with a reason). It may `extends` a shared binding. An action is one of `goto`, `reload`, `click`, `fill`, `key`, `waitUrl`, `waitText`, `waitResponse`, `run` (a command, with `expect`). A locator is `role`, `name`, `text`, `label`, `testId`, `css`, `hasText`, `within`, `nth`; a string between slashes is a regular expression. Replay over HTTP, with no browser, is an open issue; screenshots of every state on the real app are an open issue.

A **tutorial** is written against a flow: `title`, `audience`, `locale`, `appName`, `voice`, the `scenario` (the starting context), and `steps`. A step is `intro`, `chapter`, `show <state>`, `cut <state>` (jump, through a scenario the binding names), `reveal`/`focus` (nodes), `press <node>`, `after`, `card { title, body }`, `end`, and `say`. Every step is replayed on the flow, so a tutorial that no longer matches the graph stops at that step. `{word}` in `say` is a sample value or a node's visible text.

A video is an MP4 file. Its captions are WebVTT (`tutorial.vtt`, what an HTML `<track>` plays, with `tutorial.srt` beside it), and its chapters a WebVTT chapters track (`chapters.vtt`). Every video delurio makes is marked as AI-made in three ways, none of which can be turned off: a first card that says the whole video is made with AI, an imperceptible watermark in the sound and the picture, and signed, time-stamped provenance metadata (C2PA is the candidate). The watermark and the metadata are the machine-readable marking that Art. 50(2) of the EU AI Act asks of a system that generates synthetic audio and video, and the Commission's Code of Practice of 10 June 2026 describes. The disclosure is there because every voice is made from real people's recordings and may resemble one of them, which Art. 50(4) treats as a deep fake to disclose; a catalogue voice is no different from a cloned one in that, and delurio cannot tell which listener would recognise which voice (Taha, 24 Oct 2026: "Klon olmayan sesler de gerçek insan sesinden"). It opens the video, since Art. 50(5) asks for it at the latest at the first exposure, and nothing is burnt into the picture after it (Taha, 25 Oct 2026: "en başta 'videonun tamamı' diye belirtip yazsak yetmez mi"); a clip cut from the middle loses the card but keeps the watermark and the metadata. A lawyer is to confirm this reading. The video tool does not add the watermark or the metadata yet; that is an open issue.

## 8. Kit (`kind: kit`)

A kit says how the components of `ui.yaml` look in one product: `tokens` and `dark` (design tokens files in the Design Tokens Format Module 2025.10 of the W3C Design Tokens Community Group), `css` (scoped to the screen), and per component a `template` with slots (`{text}`, `{name}`, `{intent}`, `{id}`, `{children}`), flags (`{?disabled}..{/disabled}`, `named`, `conditional`), `roles` and `intents`. A kit is data: nothing in it runs or is fetched. `kit check` proves that every node keeps the role, the name and the disabled state the graph expects.

## 9. Source anchors

`source: <path>:<line>` on a state, a transition, a guard, a node, an entity or an action names the code that makes it true, relative to the application's repository root. A bare `<path>` anchors a whole file. In capture mode every element carries one; in create mode an element carries one when the code already exists.

`anchors.lock.yaml`, generated beside the graph, records what each anchor pointed at (the line, its neighbours' hashes, the commit). The check finds every line again and says whether it is unchanged, moved, changed, gone or ambiguous. A `file:line` in a description or a comment is not an anchor.

## 10. Checks and findings

| Exit | Meaning |
|---|---|
| 0 | the graph loads and, under `--strict`, nothing is wrong |
| 1 | the graph cannot be loaded: a YAML error, an unknown state, an unknown name in an expression, a non-exhaustive `always`, an unsupported construct |
| 2 | under `--strict`: an assertion is violated, a catalog key or a sample value is missing, a component has no renderer, the kit does not cover a component, or the main story cannot be walked |

**As text.** A finding is one line in the GNU error format, which compilers, editors and most CI logs read:

```
graphs/panel/nav/nav.yaml:42:5: error: the menu entry opens a page that refuses [menu-refused]
graphs/product/flow.yaml:88:7: note: step 2: CODE_CHANGED from developing, with replayed = false
```

`error` fails under `--strict` and `warning` does not; the `note` lines after a finding give its path or the places it relates to. On GitHub a problem matcher turns these lines into annotations on the pull request, at no cost and on pull requests from forks too.

**As SARIF.** `--format sarif` writes SARIF 2.1.0 (OASIS), which code scanning on GitHub, GitLab and Azure DevOps, SonarQube and the SARIF viewer for VS Code read. A finding of a graph fills every part of SARIF it has:

| delurio | SARIF |
|---|---|
| the tool, its version and every rule: id, title, description, help page, default level | `tool.driver` and its `rules` |
| a rule that stands for a WCAG success criterion | `taxonomies` (WCAG 2.2), linked from the rule by `relationships` |
| a finding: its rule, level, message, file, line and column | `result`: `ruleId`, `level`, `message`, `locations[0].physicalLocation` with its `region` |
| the graph element it is about: a state, a node, a page, an action | `logicalLocations`, e.g. `qauth.authorize/states/consent` |
| the guard or the assertion involved, the app code a `source:` names | `relatedLocations`; the graph's files under the base `SRCROOT`, the app's under `APPROOT` |
| the path that breaks an assertion: the start, each event, the context at each step, the steps inside an invoked flow | `codeFlows[].threadFlows[].locations`, with `state` for the context and `nestingLevel` for the invoked flow |
| the flow itself, and the path drawn on it | `run.graphs` and `result.graphTraversals` |
| the same finding in a later run, though its lines moved | `fingerprints` (`delurio/v1`: from the rule, the graph and the element, not the line) |
| a finding dismissed with a reason (`finding.triage`) | `suppressions`, with `justification` |
| new, unchanged or gone since a base run (`--baseline <file>`) | `baselineState` |
| a replay mismatch: the screenshot, the trace, the HTTP exchange | `attachments`, `webRequest`, `webResponse` |
| a mechanical fix, such as an anchor's new line | `fixes` |
| the finding's page in the panel; the issue it was sent to | `hostedViewerUri`; `workItemUris` |
| the files read, with their hashes; the repository and the commit | `artifacts`; `versionControlProvenance` |
| the exit code; a file that cannot be loaded | `invocations` with `exitCode`; a result of the rule `load` and a `toolExecutionNotification` |
| which graph and which check, so that uploads stay apart | `automationDetails.id` |
| the messages in Turkish (`--locale tr`) | `run.language` and a `translations` component |

The same input gives the same bytes: no times, no random ids, results in a fixed order. Every file written is valid against the OASIS schema.

There is no other machine format: no JSON of delurio's own, and nothing that only one CI reads. A converter for a CI that reads neither (GitLab's Code Quality report, Bitbucket's Code Insights) is written when a customer needs it, outside the checker. The public, pinned, offline CLI that gives both outputs is `@delurio/cli`, built from this repository; what its first release left out is [#1](https://github.com/delurio-labs/delurio/issues/1).

## 11. Writing a graph

- Model the few cases that change behaviour; a guard over hundreds of permission codes is wrong.
- Quote a YAML list item that contains `: `, or YAML reads it as a map. Quote a value inside `{ }` that contains `, ` too, or YAML cuts it there and reads the rest as a key: `{ type: final, title: "Paid, and sent" }`.
- Say every rule that must hold as an assertion; do not leave it implied by the drawing.
- In capture mode, describe what the code does today: no new feature, no fix. An invariant the code breaks is modelled as the code has it and reported, not written as an assertion that fails.
- One process, one flow. A process that runs beside another starts it (`starts:`) or waits for it (`invoke: { flow }`).

## 12. The standards it builds on

Where a standard covers a part, delurio uses it rather than writing its own. Surveyed on 22 Oct 2026.

| Standard | Used for | How |
|---|---|---|
| YAML 1.2 | every graph file | as it is |
| JSON Schema 2020-12 | the fields of entities, bodies and results (§5); a schema of every kind of graph file, so that editors check and complete them (#1) | as it is, plus the keywords of §5 |
| OpenAPI 3.1 | an action with a route is an operation (§5) | export and import: #13 |
| RFC 9457, problem details | the body of a refusal (§5) | as it is |
| SCXML (W3C, 2015) | what a flow means (§2) | a subset; export: #14 |
| WAI-ARIA 1.2, Accessible Name and Description Computation 1.2 | roles and names in `ui.yaml` (§4) | as they are |
| WCAG 2.2 | the UI rules (#4), and their taxonomy in SARIF (§10) | as it is |
| ICU MessageFormat; MessageFormat 2 | catalog messages (§4) | simple arguments today; MF2 when an app uses it |
| BCP 47 | locale names (§4) | as it is |
| Design Tokens Format Module 2025.10 (W3C community group) | kit tokens (§8) | as it is |
| WebVTT (W3C) | captions and chapters of videos (§7) | as it is; SRT beside it |
| C2PA and an imperceptible watermark | marking every AI-made video (§7), under Art. 50 of the EU AI Act | #12 |
| SARIF 2.1.0 (OASIS) | findings for machines (§10) | every part a finding has |
| GNU error format | findings as text (§10) | as it is |
| OAuth 2.0 and 2.1 (draft), RFC 9700, OpenID Connect Core, RP-Initiated Logout and Back-Channel Logout, the MCP authorization spec | signing in, signing out and agents' authority, through QAuth | cited with `spec:`, never written again |
| Model Context Protocol | the tools agents use | as it is |
| HTML custom elements | `<delurio-player>`, `<delurio-rehber>` | as they are |
| SPDX | the licence identifiers of the packages | as it is |
| Semantic Versioning | the versions of the CLI (#1) | as it is |

delurio's own, because no standard covers them: one graph whose flows, screens, pages and API are checked against each other; the checker and the guarantees it proves; the expression language (§3); navigation graphs (§6); bindings and tutorials (§7); source anchors (§9); the revision diff in words.
