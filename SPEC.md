# delurio-graph/0

This is the format of a delurio graph, as the tools in this repository read it today. Everything in it is version 0: the format settles as delurio's own graphs and its first users (qauth among them) are written in it. A part that is still open says so and names the issue that tracks it. Nothing here is a proposal kept apart from the rest; what is written is what v0 is.

The tool behaviour behind each part (the page, the video, replay, anchors, kits, the revision diff) is described in `tools/graph-viz/README.md`. This file says what the files mean.

Where a standard already covers a part, the format uses it: JSON Schema and OpenAPI for the data and the API, WAI-ARIA for screens, ICU MessageFormat and BCP 47 for words, design tokens for kits, WebVTT for captions, SARIF for findings. What delurio adds is what ties them together: one graph whose flows, screens, pages and API are checked against each other, replayed on the real app and turned into tutorials. §12 lists every standard and what it is used for.

## 1. A graph and its files

A graph is a directory of YAML files. Every file starts with the same three keys:

```yaml
schema: delurio-graph/0
kind: flow            # flow | ui | domain | nav | binding | tutorial | kit | needs | gaps
id: learning.enrollment
```

| Kind | File | What it says |
|---|---|---|
| `flow` | `flow.yaml` | the behaviour: states, events, transitions, a finite context, and guarantees the checker proves |
| `ui` | `ui.yaml` | what each state shows: the nodes of the screen, their roles, names, texts and the events they send |
| catalog | `catalog/<locale>.json` | the words of the UI, one file per locale |
| `domain` | `domain.yaml`, and any other file of the domain's directory (§5) | the data and the API: entities, actions, refusals, policies, invariants |
| `nav` | `nav.yaml` | the pages of an app, the ways between them, and who may open them |
| `binding` | `binding.yaml` | how a flow maps onto a running app, for replay and recording |
| `tutorial` | `tutorial.yaml` | an end-user video written against a flow |
| `kit` | `kit.yaml` | how the UI components look in one product |
| `needs` | `needs.yaml` | what must exist for a flow's users, each need with the evidence that it does or why it does not yet (§13) |
| `gaps` | `needs.gaps.yaml` | every need of a product's flows that is missing (§13) |
| (generated) | `anchors.lock.yaml`, `viz/`, `video/` | written by the tools; never by hand |

A flow directory holds `flow.yaml`; a flow with screens also holds `ui.yaml` and the catalog that `ui.yaml` names. It may hold `domain.yaml`, `binding.yaml`, `tutorial.yaml` and `needs.yaml`; a product's `needs.gaps.yaml` sits at the root of its graphs (§13). A domain shared by several flows sits in its own directory and each flow names it by id (`domain: learning.api`). A directory holds one domain and a domain sits in one directory, in `domain.yaml` or split across several files (§5). A navigation graph is a directory with `nav.yaml` and its catalog.

A graph file is YAML 1.2 without anchors (`&name`), aliases (`*name`), tags (`!name`) or keys that are lists or maps: a value is written where it is used, and a key is a string. No graph needs them, and a YAML reader can take minutes over a small file that uses them, so a file with one cannot be loaded ("unsupported YAML", at its line). `@delurio/cli/yaml` reads a file this way.

A flow with no screen (an HTTP endpoint, a webhook, an MCP tool, a CI job) has no `ui.yaml`: it loads and is checked without one, and its page draws no screens. What such a step answers the outside world is written on its state as `response` (§2).

### Common keys

| Key | Meaning |
|---|---|
| `title`, `summary` | the graph in plain words, for people |
| `status` | `proposed`: written before or apart from the code, nobody has approved it. `captured`: read from existing code (capture mode, every element carries `source:`, §9). `approved`: the owner reviewed and accepted it |
| `source` | on any element: `<repo-relative path>:<line>` or a bare `<path>`, the code that makes the element true. See §9 |

### Text

The graph is written in one language, given by the project. Titles, `say` sentences, descriptions and assertion sentences are in that language. Every word a user of the product sees lives in a catalog, one file per locale; the graph refers to it by key. Identifiers (states, events, guards, effects, nodes, catalog keys) are English camelCase; events are UPPER_SNAKE. An exit (a final state) may instead be named after what the outside world is told, in snake_case, followed by its cause when several exits tell the same thing: `signed_out`, `invalid_token.dpop_bound`, `invalid_token.expired`. A protocol's errors are snake_case, so its exits read as its errors do.

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
| `type: final` | an exit; it has no transitions. Its id may be `<snake_case>` or `<error>.<cause>` (§1, Text) |
| `redirect`, `code`, `carries` | on an exit of a web protocol flow: `redirect: success \| error \| never`, the error `code`s, and what a success carries back |
| `leavesPage: true` | on an exit: the person left (closed the page, went elsewhere) |
| `response` | on a step with no screen: what the outside world sees, as data: `status`, `code`, `redirect`, `headers`, `body`, or the fields of a tool's answer. The page shows it beside the step, and a replay over HTTP will read it |
| `source` | the code that makes the state true |

A transition is a target state id, or `{ target, guard, effect: [..], say, starts, source }`, or a list of those: the first whose guard holds is taken. `say` is the transition in words; `starts` is under "Several flows" below.

### Events

- A state with `invoke` ends with `DONE` (success) or `FAIL.<class>`, where the class is a key under `failures` for that state; `failures.<state>.from` names the domain action, and each class lists the refusal codes of that action it stands for, or the transport codes `network`, `timeout` and `5xx` (§5).
- `NAVIGATE` is the person leaving the page.
- An event caused by someone else (a course team, a webhook, another flow) is an event like any other; its name and `say` make clear who causes it.

### Main story, scope, decisions

- `main` is the story the page tells first and the path a tutorial and a replay take by default: the events, in order, from the `given` start. A plain list of events means no `given`. `--strict` fails when the events cannot be walked. Without `main`, the first success exit that passes a screen is the main story.
- `scope` says what the view shows (`shows`) and how to read what it does not draw. `world: open` means that what is not drawn is not ruled out; only `assertions` and `outOfView` are binding. `world: closed` means the flow is complete. What must exist for the flow's users, whether the flow draws it or not, is in its needs (§13).
- `decisions` are the questions still open in this flow, each with a recommendation and the states it affects.

### Assertions and the checker

A configuration is a state together with a value for every context variable. The checker starts from every initial configuration and follows every transition whose guard holds, applying its effects, breadth first. An `always` state moves on at once. Every reachable configuration is visited once.

Each assertion is `never: <expression>`: it holds when no reachable configuration satisfies the expression. A violation is reported with the shortest event path from a starting situation. Under `--strict` a violated assertion exits 2.

`check` gives an error (`state-unreachable`) for a state that no reachable configuration is in: no transition leads to it, or those that do are never taken from a start. It gives one (`state-no-way-out`) for a state that is not an exit (`type: final`) and has no `on`, `always` or `invoke`, so that a flow that reaches it stops there without ending. Both are given at the state and fail `--strict`; the page marks an unreachable state too. Transitions that no situation takes are shown on the page and fail nothing yet; that is [#2](https://github.com/delurio-labs/delurio/issues/2).

`cover` in `@delurio/cli/model` gives event paths that together pass every reachable state, or take every reachable transition, from the same starts the checker explores (or the ones a replay is bound to). Each path is a shortest way to the state or transition it is kept for, so it repeats a state only on the way to something not yet covered; the longest are kept first, and a path that covers nothing new is left out. Each has an id, a hash of its start and its transitions, and lists what it covers, and the result lists what no situation reaches. The same flow gives the same paths in the same order. A replay walks them (`--cover`, §7). Why a transition cannot be reached, a coverage report of the walks, and the command that fails on what they did not reach are [#4](https://github.com/delurio-labs/delurio/issues/4).

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
task: { key: Authorize.task }       # optional: the flow's name as its users know it
focus: { <state>: <node> }          # optional: the node that has focus once a step has led into the state
nodes: { <node>: { .. } }
edges: [ { labelledBy: [node, node] }, { triggers: [node, state] }, { returnsFocus: [dialog, node | [node, ..] | opener] } ]
rules: [ { id, rule } ]
```

| Node key | Meaning |
|---|---|
| `type` | `page`, `text`, `action`, `field`, `list`, `status`, `alert` |
| `component` | a kit-neutral component: `Page`, `Heading`, `Text`, `Button`, `LinkButton`, `TextField`, `Checkbox`, `Radio`, `Select`, `List`, `ListItem`, `Status`, `Alert`. A kit maps it to the product's own (§8). A `TextField`'s, a `Checkbox`'s, a `Radio`'s and a `Select`'s `text` is its label, and the control is named by it. A `Checkbox` (role `checkbox` or `switch`) and a `Radio` (role `radio`) say whether they are checked with `checked`; a `Select` (role `combobox`, the HTML `<select>`) is a choice of one option, and its `value` is the option it shows chosen. A `ListItem` whose role is `checkbox` or `radio`, in a `List` whose role is `group` or `radiogroup`, still says the same |
| `role` | the ARIA role the node exposes |
| `intent` | for actions: `confirm`, `dismiss`, `secondary` |
| `name` | the accessible name: `{ key, params }` from the catalog, `{ labelledBy: <node> }`, `{ literal }`, or `{ oneOf: [keys] }` |
| `text` | the visible text, same forms. Kept apart from `name` so that "the visible text is in the name" (WCAG 2.5.3) is checkable |
| `in`, `unless` | the flow states the node is shown in, and those it is not |
| `when` | a guard or context flag that must hold for the node to show |
| `disabledIn` | states where the node is shown but disabled |
| `disabledWhen` | an expression over the context (§3): where it holds, the node is shown but disabled, in any state that shows it |
| `emits` | the event the node sends when pressed |
| `initialFocus` | on a page: the node that has focus when it opens (Focus, below) |
| `repeat` | the node is drawn once per item of this data (`request.scopes`) |
| `frame` | for a page: whether it may be framed (`never`) |
| `value` | on a field: what it holds when it is shown, until someone types in it: `{ key, params }`, `{ literal }` or `{ oneOf }`; `{ literal: "" }` is an empty field |
| `checked` | on a node whose role is `checkbox`, `menuitemcheckbox`, `menuitemradio`, `option`, `radio` or `switch`: whether it is checked (an `option`: selected) when it is shown, as an expression over the context (§3), or `true` or `false` |
| `script` | on a page: how much of it works without JavaScript. `optional`: the page and its actions; `actions`: the page is drawn and read without it, and only its actions need it. Without `script`, the page needs JavaScript |
| `source` | the code that draws it |

`task` is a catalog message that names the flow as its users know it, in their words and their language: "Edit a note", "Invite a member". Rehber names the task by it in the page's language, and a list of tasks or a video's title can show it. The graph's `title` is in the graph's language, for the people who write and review it; `task` is in every locale. `check` gives an error (`task-locale`) where `task`'s key is missing from, or empty in, the catalog of any locale. A flow without `task` is not held to one; a tool that names it to its users then has no words of theirs for it.

`changes` is a catalog message that tells the flow's users, in their own words, what the flow changes and what follows from it: what is created, sent, charged or removed, who sees it, and whether it can be undone. Rehber shows it before it guides the flow and again at the step that makes the change, and it guides a flow whose steps write something only when the flow has one (rehber.install, rehber.guide; Taha, 24 Oct 2026: "readonly olmayan her akış için nelerin değişeceğini ve ne etkilerinin olacağını okuyabilecekleri bi not olsun"). The step that makes the change is found from the graph: the one that leads into a step invoking an action that writes. `graph-viz --strict` fails a flow whose steps invoke a writing action of its domain and whose ui has no `changes`, and a `changes` key missing from any locale's catalog (`tools/graph-viz/README.md`).

**Proposed, for the owner to decide; not part of v0 until it is decided.** A `GET` that a policy lists writes only when it does more than record its call. A policy names everything its actor may do, reads and writes together, so its `write` says what some of the actions of its `do` write, not that each of them does; and a `GET` reads (§5). Such a `GET` writes when one of its `sideEffects` is other than an audit record, `audit:<event>`. An audit record is what the service keeps of the call itself, who read what and when, as a server keeps its access log (RFC 9110 §9.2.1): it is no change that the flow's users make or are told of. So a flow that only reads, through a `GET` that writes an audit record each time it is called, needs no changes note, and one whose `GET` also writes something else, such as the session a sign-in callback opens (`sideEffects: ["a session for the person"]`), still does. An action whose method is not `GET`, and a step inside the service, write as before, whatever their side effects: an audit record beside a write is still a write. Until the owner decides, `check` reads actions so.

A `role` is a WAI-ARIA 1.2 role, and `name` is the accessible name that Accessible Name and Description Computation 1.2 gives the node; `kit check` reads both from the browser's accessibility tree.

A node is shown in a state when the state is in `in`, not in `unless`, and `when` holds. `edges` records relations the page and the checks use: a node labelled by another, an action that leads to a state, the node a dialog gives the focus back to. `rules` are written but no tool reads them yet; checking them, together with labelled dialogs, confirmation before a destructive action and events handled where shown, is [#3](https://github.com/delurio-labs/delurio/issues/3). The focus return of #3 is partly answered: a dialog says where the focus returns with `returnsFocus` (below), and `check` holds that it starts at a dialog and gives the focus to a node shown where the dialog closes. That every dialog says it is still #3.

### Focus

A page that opens gives the focus to its `initialFocus`. `focus` and `returnsFocus` say where it goes after that, when someone acts or a step ends. A page opens when the flow enters a state that shows it from a state that does not, or starts in a state that shows it, however the browser got there: by a step inside the app, or by loading the page anew (a link from an e-mail, a reload, the way back from another site). Its `initialFocus` holds in both; an app that leaves the focus at the document's start after a load does not meet it. The flow moves from one state it stops in to the next: a state with `always` is passed at once, so the focus is never said there. When the flow moves from one state into another, the node that has focus in the new state is the first of these that applies:

1. the node of `{ returnsFocus: [<dialog>, <node>] }` in `edges`, when the dialog closes (the state before shows it and the new one does not) and the new state shows the node. The dialog is a page node whose role is `dialog` or `alertdialog`, and the node is usually the one that opened it, as the dialog pattern of the ARIA Authoring Practices Guide asks. Where several nodes open the same dialog, the second item is the list of them, `[<dialog>, [<node>, ..]]`, or `opener`, `[<dialog>, opener]`: the focus goes back to the node that opened the dialog this time, one of those listed for a list (opener, below);
2. the `initialFocus` of a page that opens and has one; when a dialog opens over a page, the dialog's;
3. `focus.<state>`, the node that has focus when the flow enters that state from another one: on a page that stays open, the alert a failed step shows, or the status that says a step is done.

When none applies, and when the flow moves from a state to itself, the graph says nothing of the focus. `focusAfter` in `@delurio/cli/graph` gives this order.

The node that opened a dialog is the one pressed to open it: a node shown in the state before the dialog opened, whose `emits` is the event of the transition into the state that shows the dialog (past states with `always`). A dialog that edits any of four fields, each with its own Edit button, gives the focus back to the Edit button that was pressed. The graph cannot say which one that was, as the four lead to the same state; a replay knows the node it pressed, and gives it to `focusAfter` as its fourth argument. Without it, `focusAfter` gives the one node of a list that the new state shows, and nothing for `opener` or for a list of which the new state shows several. `opener` is a word of the format, not a node: no node is called `opener`. A replay checks, in each state it stops in, that the node the graph gives has the focus: the browser's focused element is that node, found by its role and name or by its binding (§7).

A state that `focus` names that is not a state of the flow keeps the graph from loading. `check` gives an error (`focus-not-shown`) where `focus` gives the focus to a node its state does not show (by `in` and `unless`; a replay reads `when`), or a `returnsFocus` to a node no state the dialog closes into shows, and one (`returns-focus-not-dialog`) where a `returnsFocus` starts at a node that is not a dialog. For a list, each node it lists is held so; for `opener`, each node that opens the dialog, and a dialog that no node opens is `focus-not-shown` too.

### What a control holds, and pages without JavaScript

`value` is what a field holds when it is shown, until someone types in it. It takes the forms of a node's words except `labelledBy`: `{ key, params }` from the catalog, `{ literal }`, or `{ oneOf }`, and `{ literal: "" }` is an empty field. A replay compares it in each state that shows the field, up to the step at which its walk types in that field. `checked` says whether a control is checked, or for an `option` selected, in each state that shows it: an expression over the context there (§3), or `true` or `false`. A control someone checks during the flow is checked by an event, and the expression reads what that event's effect sets; one that the flow finds already checked is said over the context, such as the radio of the role a member already has (`checked: "role == 'admin'"`). A replay compares it in each state that shows the node. On a node drawn more than once (`repeat`, or words given as `oneOf`), `value` and `checked` hold for each control it draws; controls that differ are written as separate nodes.

`disabledIn` lists the states in which a node is shown but disabled. `disabledWhen` disables it by what the context holds, in every state that shows it: a Continue button disabled until a VAT number is typed (`disabledWhen: "not vatGiven"`, where typing it is an event whose effect sets `vatGiven`), or until a box is ticked (`disabledWhen: "not accepted"`, beside the box's `checked: accepted`). A node is disabled where either says so. A replay compares it in each situation it stops in, from the context it has walked to there. A `disabledWhen` that does not parse, or names anything but a variable, a guard or `state`, keeps the graph from loading, as `checked` does.

`script` on a page says how much of it works without JavaScript. With `script: optional` the page and its actions work without it: its forms are posted and its links followed by the browser itself, and JavaScript only adds to them. With `script: actions` the page is drawn and can be read without JavaScript, as a page rendered on the server is, but its actions need it. Without `script` the page needs JavaScript, and the graph says nothing of it without. A replay with `--no-js` (§7) walks the flow a second time with JavaScript off. In a state whose page is `optional` it checks what the state shows, as on the first walk, and takes the next step there. In a state whose page is `actions` it checks what the state shows, and takes no step from it: the walk ends there, as it does where a step leads to a page without `script`.

While scripts do not run, an action of a page marked `actions` is not there, or it is there disabled and says why. A control that looks ready and does nothing when pressed, a dead button, does not meet this. The app may leave the control out of the page it draws on the server and add it when its script runs, or draw it disabled (`disabled`, or `aria-disabled="true"`) with an accessible description that gives the reason, such as that it needs JavaScript (a `<noscript>` note that `aria-describedby` points at). So in such a state the `--no-js` walk checks each action node the state shows (`type: action`): it is not visible, or it is disabled and its accessible description (Accessible Name and Description Computation 1.2) is not empty. One that is visible and enabled, or disabled with no description, is a mismatch.

The `schema` rule refuses `value` on a node that is not a field, `checked` on a node of another role, `script` on a node that is not a page, and a node called `opener`. A `checked` or a `disabledWhen` expression that names neither a variable, a guard nor `state` keeps the graph from loading. A `value` from the catalog is held as a node's text is (`catalog-key-missing`, `sample-missing`).

### Catalog

A JSON object from key to an ICU MessageFormat message: the syntax of the catalogs apps already keep (ICU, FormatJS, next-intl), so a captured catalog is copied as it is. Today the tools fill simple arguments: `{param}` is filled from the node's `params`, which name sample or data values. A `plural` or `select` argument is read when a catalog first needs one. MessageFormat 2 (Unicode, stable since CLDR 47) is read when an app's own catalog is written in it.

One file per locale; `ui.yaml` names the one the page uses (`catalog`), and `locales` lists every locale its screens are written in, each in a file of its own beside that one, `<locale>.json`. A `ui.yaml` that names no catalog, or names one that is not there or is not JSON, cannot be loaded. `check` holds every locale's catalog to the same words: a key that a screen uses (a node's `name`, `text` or `value`) and that a locale's catalog lacks is an error (`catalog-key-missing`) at the key in `ui.yaml`, and so is a locale of `locales` whose file is not there. A placeholder with no sample value (`sample-missing`) is looked for in the catalog `ui.yaml` names. A locale is a BCP 47 tag, in the file name (`en.json`, `tr.json`, `pt-BR.json`) and in `locales` and `locale`.

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

**The data.** An entity's `fields` give each field as a JSON Schema (2020-12), and `required` lists the fields that must be present: an entity is the JSON Schema `{ type: object, properties: <fields>, required: <required> }`. `null` is a type like any other: an optional time is `{ type: [string, "null"], format: date-time }`, and an enum that may be empty lists `null` among its values. A `body` or a `result` written out is a JSON Schema too. An entity may carry `examples`, JSON Schema's own keyword: records of it as the app keeps them, each an object of its fields (decided on 1 Nov 2026). delurio adds these keywords, which a JSON Schema validator leaves alone:

| Keyword | On | Meaning |
|---|---|---|
| `key` | an entity | the fields that identify it; `[id]` when left out |
| `references` | a field | it holds the key of another entity |
| `unique` | a field | `true`, or `case-insensitive` |
| `sensitivity` | a field | `credential`, `personal` or `internal`: what may be shown, logged or kept |
| `lifecycle` | an entity | its states, as `from -> to: why` sentences |

A capture may carry more words of its own on a field (`owner`, `note`, `editableBy`, `matching`, `opaque`). They are kept and compared, and no tool reads them.

An action, a refusal and an invariant may carry `note`: the author's own words, which no tool reads. An invariant the code does not keep is still written, with `holds: false` and the `finding` that reports it: the finding's id in the graph's report, such as `MS-7`. A capture describes the code, so the invariant says what should hold and the finding says where it does not. Decided on 25 Oct 2026.

**The API.** An action with a `route` (`METHOD /path`) is one OpenAPI 3.1 operation: its name is the `operationId`, `needs` its security requirement, `query` (a JSON Schema object) its query parameters, `body` its request body, `result` its response (`returns` an entity, a `redirect`, a `status`, or a JSON Schema), and each refusal one more response. When a refusal has a body, it is RFC 9457 problem details with the refusal code as its `type`. So a domain can be written out as an OpenAPI document, and an app that already publishes one names it at the top (`openapi: <file>`) and each action's operation by `operationId`, instead of copying them. An action with `serverFn` is a server function (below), and an action with neither is a step inside the service. `needs`, `policies`, `sideEffects`, `refusals.<code>.redirect` and `invariants` are delurio's own: no standard says who may do what to which data, or what must always hold.

**Server functions.** Full-stack frameworks let a product's own pages call code on its server as if it were a function: TanStack Start's `createServerFn`, or React's server functions. The framework gives each one an address of its own and makes the call from the product's pages; it is not a public HTTP API, and nothing else is meant to call it. Such an action says so with `serverFn` and its HTTP method, `GET` or `POST`, in place of a route:

```yaml
actions:
  readNote: { serverFn: GET, needs: session, body: { type: object, required: [id] }, result: { returns: Note } }
  restoreNote: { serverFn: POST, needs: session, body: { type: object, required: [id] }, result: { returns: Note } }
```

- `GET` reads: it changes nothing, so the framework may call it ahead of time (a page's loader, a preload) or keep its answer. `POST` may write, and every server function that changes something is one.
- Its input is its `body`, whatever its method: a framework puts a `GET`'s input in the address under a name of its own, so a server function has no `query`. `needs`, `result`, `refusals` and `sideEffects` mean what they mean for a route. The function is reached over HTTP like any request, so what it `needs` is checked by the function itself, never only by the page that calls it.
- It is no OpenAPI operation: the OpenAPI document of a domain ([#8](https://github.com/delurio-labs/delurio/issues/8)) leaves it out, and it has no `operationId`.
- An action has a route or `serverFn`, never both. An address that the browser itself opens, such as a link in an e-mail, a sign-in callback, or a link that sends the browser on to another site, keeps its `route` even when a server function answers it: the address is what is called, and others may link to it, so it is part of the API.

`check` gives an error (`server-fn-http`) at the `serverFn` of an action that also has a `route`, a `query` or an `operationId`, in whichever of the domain's files each is given, with a note where the other key is; a method other than `GET` or `POST` is a `schema` error. Decided on 12 Nov 2026.

This form was decided on 22 Oct 2026. Graphs captured before it may still give fields as `{ kind: text }`; they are converted when a tool first reads fields. The OpenAPI export and import are [#8](https://github.com/delurio-labs/delurio/issues/8).

**Several files.** A domain may be split across the files of its directory: every `*.yaml` there that says `kind: domain` is a part of it (a file named for another kind, such as `flow.yaml`, never is). Each part starts with `schema`, `kind` and the domain's `id`, and the parts are read as one domain, `domain.yaml` first and then the others by name:

- maps merge key by key, at any depth: one file may give an entity's fields, and another add its actions or mark one of its fields `sensitivity: credential`;
- the lists `policies` and `invariants` join, in that order; an invariant's `id` is given once;
- any other value (a string, a number, `true`, a list such as `required` or `lifecycle`) is given in one file only.

A value given in two files is an error of `check` at the second one's line (`domain-twice`); so is a file whose id is not its directory's domain, and a domain whose files sit in two directories (`domain-place`). A split keeps a large domain one entity per file, and lets a part generated from the code ([#6](https://github.com/delurio-labs/delurio/issues/6)) and a part written by hand sit side by side; how a generator marks the files it owns is decided there. Decided on 1 Nov 2026.

`check` validates every file of a domain against its schema and reads its files as one, the `changes` rule (§4) reads its actions (one writes when its route or its server function is not a `GET`, when a policy lets someone `write` with it, or when it is a step inside the service that returns an entity; a `GET` that a policy lists is under the proposal in §4), the revision diff compares it, anchors lock its `source:` keys, and the page of a flow that names it lists what each of its steps calls and how: an HTTP operation and its route, a server function and its method, or a step inside the service. `check` also reads the domain a flow names, and holds the names the flow gives to it (The names a flow gives, below). Checking secrets (`sensitivity`) and what a policy lets each actor do is [#5](https://github.com/delurio-labs/delurio/issues/5). Generating it from Drizzle and Zod is [#6](https://github.com/delurio-labs/delurio/issues/6); a registry that every API operation and route is covered by a graph is [#7](https://github.com/delurio-labs/delurio/issues/7).

**The names it gives.** A domain's names are its own: `check` holds each name a domain gives to what the domain has, all its files read as one, and gives each finding in the file that gives the name, at its line. An action's `result.returns`, and a `references` on a field of an entity or anywhere in a `body`, a `query`, a `result` or a refusal's `body`, name an entity of the domain, as the keywords above say; one that names no entity of it is an error (`entity-unknown`). Where another domain among the graphs checked has an entity of that name, a note says where: a domain does not name another one's entities, and a large one is split across the files of its directory instead (Several files, above). An entity's `key` and `required` name its own fields; one that is not a field of the entity is an error (`field-unknown`), and an entity none of whose files gives `fields` yet is not held to them. A policy's `do` names actions of the domain, and its `read` and `write` name entities; one that names what the domain does not have is an error (`policy-unknown`).

**The names a flow gives.** A flow's names are held to its domain as the domain's own are. A state's `invoke: { action }` names an action of the domain the flow names, and so does `failures.<state>.from`; one that names no action of it is an error (`invoke-unknown`), and so is a flow whose states invoke actions while the domain it names is not found beside it (under the nearest folder called `graphs`). Each class under `failures.<state>` lists codes of that action's `refusals` (§2), or transport codes (below); any other code is an error (`refusal-unknown`). A state that invokes an `external` or a `flow` is not held to the domain. Decided on 16 Nov 2026 (d-1116-10): these fail `--strict` from the release that first gives them, with no release in which they only warn.

**Transport codes.** A call can fail without the action answering it with a code of its own: nothing comes back, or the server fails. Three codes stand for these failures, and a failure class may list them though the action has no refusal of that name:

| Code | The call fails because |
|---|---|
| `network` | no answer comes back: the address cannot be reached, the network is down, or the connection breaks before the answer |
| `timeout` | the answer does not come within the time the caller waits for it |
| `5xx` | the server answers with a status from 500 to 599 that is not one of the action's refusals: it failed, or it is not available |

They are the only ones. A failure that the action answers with a code of its own, such as a protocol's `server_error` or a service's `UNAVAILABLE`, is one of its refusals, as `UNAVAILABLE` and `server_error` are in `examples/qauth-authorize`. An action may also give a refusal one of these three names, and that refusal is then its own.

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

A **binding** ties a flow to a running app for replay and recording: `apps` (base addresses), `browser`, `data` (JSON or dotenv sources, read again after each command, secrets masked in reports), `signIn`, `accounts`, `scenarios` (each a `given` start, the account, the page to `open`, `setup` and `teardown`, `javaScript: false` for a walk with JavaScript off, and `enter`, the way the walk comes in), `events` (the actions that cause an event no node on screen sends), `effects` (the checks of what a step changes outside the screen), `states` (a URL glob or a text a state must show, the server `call` a step makes, the account `as` whose screen it is, and `before` actions) and `nodes` (a locator for a node that role and name cannot find, or `before` actions such as opening a menu, `count` for how many elements it finds, or `skip` with a reason). It may `extends` a shared binding. An action is one of `goto`, `reload`, `click`, `fill`, `key`, `waitUrl`, `waitText`, `waitResponse`, `run` (a command, with `expect`), and any action may carry `as`. A locator is `frame`, `role`, `name`, `text`, `label`, `testId`, `css`, `hasText`, `within`, `nth`; `frame` is the CSS selector of the iframe the element is drawn in, such as a payment provider's hosted form, and is looked in first; a string between slashes is a regular expression.

`as: <account>` on an action does it in that account's own browser, with its own cookies, opened and signed in the first time it is used: a step another person takes, such as a second member who approves or an invitee who accepts, is bound to actions `as` that person, and the walk goes on in the first browser after them. A state may carry `as` too: its screen is that person's, so it is checked, and its nodes are pressed, in their browser, while a state without `as` is the scenario's own account's. A state's `before: [actions]` runs when the walk reaches the state, before it is checked, such as that person opening the link an e-mail brought them. So a step another person takes after arriving by a mailed link is bound to its state: `as` the invitee, `before` opening the link. `effects: { <id>: [actions] }` checks what a step leaves outside the screen, such as an e-mail sent or a row written. `<id>` is an effect of the flow (§2) or an action of its domain (§5): the actions run after the walk takes a transition that applies that effect, or after a state that invokes that action ends with `DONE`. They are checks, typically a `run` with `expect` that reads a mail catcher's API or queries the database, and one that fails is a mismatch at that step.

`count: <number | "{data.path}">` on a node says how many elements its locator finds, as a number or a value of the binding's data: for a `repeat` node (§4), how many items it shows, and for any other node, how many there are, such as four radios and no fifth. Without it a `repeat` node shows at least one item, and more are accepted. `call: <url glob>` on a state names the server call its `invoke` makes. The replay holds the call until it has checked what the state shows, so a step in progress (its status, the controls in `disabledIn`) is seen as it is, and then lets it go; on a walk through `FAIL.<class>` it compares the call's HTTP status with the `status` of the refusals that the class stands for (§2, `failures`; §5). A scenario with `javaScript: false` is walked with JavaScript off, through the pages marked `script` (§4).

The binding's `data` is read when a walk starts, after its `setup`, and again after each `run`, so a command can hand the next action a value it has just written, such as the link or the code of an e-mail that it read from a mail catcher. A source whose file does not exist yet is allowed: the walk fails, naming the file, only when an action uses a value of it (`{<source>.<path>}`) while the file is still missing.

A walk reads every screen in one language: the binding's `browser.locale`, a BCP 47 tag the browser is set to, or the replay's `--locale`. Each screen is checked against the flow's catalog of that language (§4), found by the whole tag first and then by its language alone: `tr-TR` reads `catalog/tr.json`, and `pt-BR` reads `pt-BR.json` before `pt.json`. A locale the flow has no catalog for is refused, never checked in another language. A flow the walk enters (`enter`) and a navigation graph are read in the same language. `--locale` may be given more than once: the walks are made in each locale, each with a report of its own. A walk keeps to its language: after a step that switches the app's language, the next screens are still read in the walk's. Without either, the screens are read against the catalog `ui.yaml` names.

`browser.ignoreHTTPSErrors: true` opens a page served with a certificate no authority signed, such as an app on a self-signed `https` origin in development; without it, such a page fails the walk.

What a person types, or what a scenario's `setup` put in place, the graph may leave to chance (`havoc`, §2), while on the app it is what the scenario made it. So a walk keeps, through a `havoc`, the value its scenario's `given` gives each variable that starts from any value: a refusal is walked from a scenario that types the invalid value, and a way that needs the value changed there is not bound from that scenario (it is listed as not bound), rather than walked from one that set up the opposite. A variable that starts from a fixed value takes the values the `havoc` allows, as in the graph.

Where several nodes on the screen send the event a step takes, such as two radios of one choice, the binding's `events.<EVENT>` says which is pressed, when it gives one; otherwise the first of them in `ui.yaml` that the state shows enabled is pressed.

The **replay** walks a flow on the app through its binding and checks each state it stops in: the nodes the graph shows there (their role, name and text, whether they are disabled, which one has the focus, what a field holds and whether a control is checked, §4), the state's `url` and `text`, and the `effects` of the step that led there. `replay <flow dir> --bind <binding>` walks the main story (§2). `--to <state>` walks the shortest way to that state that the binding can drive. `--cover states` walks paths that together pass every reachable state, and `--cover transitions` paths that together take every reachable transition, from the binding's scenarios and through the steps it can drive (§2, `cover`). `--no-js` walks the pages marked `script` a second time with JavaScript off: those marked `optional` with their actions, those marked `actions` read only, their actions absent or disabled with a reason (§4). `--absent` also checks that a node the graph does not show in a state is not visible there; without it, the replay checks only what the graph shows, and a node it leaves out of a state may be there or not. A state or a node that only missing needs are drawn as is planned, not built: every walk leaves it out, and the replay lists it with those needs (§13).

`enter` says how a scenario's walk comes into the flow, after the scenario opens its page (`open`). `enter: { nav: <nav graph dir>, bind: <nav binding>, entry: <from>-><to> }` presses that entry of a navigation graph (§6), found as its own replay finds it (below), its menu opened first, and the address must become the route of the entry's page; `bind` may be left out when the entry's label alone finds it. `enter: { flow: <graph dir>, bind: <its binding>, given: { .. }, event: <EVENT> }` walks another flow from `given`, by the shortest way its binding can drive, to the transition on `<EVENT>` whose `starts` names this flow (§2, Several flows), and checks its screens on the way; what that `starts` hands over must be this scenario's own `given`. The paths are relative to the binding's file. On arrival the focus must be on the `initialFocus` of the entered flow's first screen, past its rules and server calls, and the flow's own walk then goes on from its initial state, so that every walk of the flow can begin where a person comes in.

A binding may tie a navigation graph (§6) to the app instead of a flow. It names the graph with `nav: <id>` in place of `flow`, has the keys a flow's binding has for the app (`root`, `extends`, `apps`, `browser`, `data`, `signIn`, `accounts`), and gives `samples` for the `:params` of the routes; a page is opened at the first app and its route. Without `personas`, each persona of the graph signs in as the account named like it. `personas.<persona>: { as, setup }` lets one account stand for several personas: `as` names the account, and `setup`, commands (`run`) done before that persona's visits, gives the account the persona's facts, such as its role. A `setup` at the top, also commands only, runs once before the first persona, for instance to sign the people up. The other keys say where the replay finds what the graph names, and each may be left out. `frames.<frame>` gives `find`, the frame's chrome, where its `menu` entries are; `page`, where a page of the frame draws its own entries (`link`, `button`); and `user` and `picker`, each `{ open: [actions], find }`: what opens the person's menu or the switcher, and the open menu, where their entries are found. `pages.<page>` gives `refusal`, a locator for the words in which the page says no (the graph says them in prose, so a refusal is checked only with one); `shows`, what proves the page opened, absent when it refuses; `samples`, the `:params` of its route for this page, over the binding's own, so that two pages on one route open at items of their own (a revision in review and one decided); or `skip` with a reason. `entries.<from>-><to>` is keyed by the entry's ends, as a code comment cites it (`N:<from>-><to>`), `<from>` being a page or `frame:<frame>`, followed by its `how` or its label key where two entries share their ends (`frame:workspace->start picker`). It gives a locator for the entry's element, where its label is not its accessible name or the entry needs an item (`data: true`); `skip` with a reason; or `follow`. Without a locator in the binding, an entry is found by its label, as the accessible name of what its `how` draws: a link (`menu`, `link`), a button (`button`) or a menu item (`user`, `picker`), in the frame's `find`, its `page` or the open menu.

`replay <nav graph dir> --bind <binding>` opens every page the binding can open, as every persona it binds, and checks what the graph says that persona meets there: the page stays at the address it was opened at, and it opens, or refuses where its `opensFor` does not hold (its `refusal` shown and its `shows` not); each bound frame's chrome is there on the frame's pages and absent on the others; and each entry of the page or of its frame is shown where its `when` holds for the persona and absent where it does not. An entry that needs an item is checked only where the binding gives its locator. `--persona <id>` and `--page <id>`, each given once or more, keep the walk to those personas and pages. `--follow` also takes each entry shown, once per persona, and checks that it lands on its page's route; a `button` acts, so it is taken only with `follow: true`, and `follow: false` keeps an entry from being taken at all (one that signs out or deletes). `--no-setup` skips the top-level `setup` and uses the people of the last run again. `--locale`, once or more, reads the pages in that language: the entries' labels from the navigation graph's catalog of it, found as a flow's is.

Replay over HTTP, with no browser, is an open issue; screenshots of every state on the real app are an open issue.

A **tutorial** is written against a flow: `title`, `audience`, `locale`, `appName`, `voice`, `stage`, the `scenario` (the starting context), and `steps`. A step is `intro`, `chapter`, `show <state>`, `cut <state>` (jump, through a scenario the binding names), `reveal`/`focus` (nodes), `press <node>`, `after`, `then`, `card { title, body }`, `end <state>` (the exit the flow ends at, or the screen the video closes on), and `say`. Every step is replayed on the flow, so a tutorial that no longer matches the graph stops at that step. `{word}` in `say` is a sample value or a node's visible text.

`then { <variable>: <value> }` says how a step comes out where an effect leaves it to chance (`havoc`, §2): what the host answered, what a folder held. Of the cases the step's transitions can take, the step takes the one in which these variables have these values. A step without `then` takes the model's first case, and a `then` that no case of the step matches stops the tutorial at that step. So a tutorial can go the way a person meets most, such as a host that lists their repositories, when the graph also allows every other answer.

`stage` says what the video shows: the flow's screens (`screen`, the default), or its diagram (`diagram`), for a flow with no screens, such as two programs talking or a job in the background. On the diagram the camera goes from state to state, and the tutorial's own way is drawn as the story, whether or not it is the flow's `main` story; the diagram holds every transition, failures included. No state is passed on its own there: `show <state>` and `end <state>` walk the flow to their state along its normal way (a transition with no event, then `DONE`) and stop the tutorial when that way does not reach it, and every other event is given with `after`. `press`, `reveal` and `focus` point at nodes a diagram does not have, and stop it too. An `end` on the diagram needs no `card`; one it has is shown after the ending.

`check` reads the `tutorial.yaml` beside a flow and replays it on the flow as the video does, from the start its `scenario` gives: a value of its list for each variable that starts from any value, and none that differs from a fixed `initial`. On the screens, a step that shows no screen passes on its own along its normal way (a transition with no event, then `DONE`), and one with no such way waits for an event given with `after`; on the diagram, `show` and `end` walk as above. A tutorial that no longer matches its flow is an error (`tutorial-mismatch`) that fails `--strict`, at the step where it stops: its `flow` is not the flow beside it; a step names a state, a node, an event, a variable or a value the graph does not have, or a `{word}` that is neither a sample value nor a node with visible text; `show` names a state the flow is not in; `cut` names a state no reachable situation is in, with the nodes it `reveal`s shown; `press` names a node the state does not show, or disables, or that sends no event the state handles; `after` names an event the state does not handle; `reveal` or `focus` names a node the step's screen does not show, or `focus` one shown only `when` something holds and not revealed; `end` names a state the flow is not in, or, given no state, the flow is not at an exit; or a `then` matches no case of its step. Every name is looked for on every step, and the steps are replayed once all are found.

A video is an MP4 file. Its captions are WebVTT (`tutorial.vtt`, what an HTML `<track>` plays, with `tutorial.srt` beside it), and its chapters a WebVTT chapters track (`chapters.vtt`). Every video delurio makes is marked as AI-made in three ways, none of which can be turned off: a first card that says the whole video is made with AI, an imperceptible watermark in the sound and the picture, and signed, time-stamped provenance metadata (C2PA is the candidate). The watermark and the metadata are the machine-readable marking that Art. 50(2) of the EU AI Act asks of a system that generates synthetic audio and video, and the Commission's Code of Practice of 10 June 2026 describes. The disclosure is there because every voice is made from real people's recordings and may resemble one of them, which Art. 50(4) treats as a deep fake to disclose; a catalogue voice is no different from a cloned one in that, and delurio cannot tell which listener would recognise which voice (Taha, 24 Oct 2026: "Klon olmayan sesler de gerçek insan sesinden"). It opens the video, since Art. 50(5) asks for it at the latest at the first exposure, and nothing is burnt into the picture after it (Taha, 25 Oct 2026: "en başta 'videonun tamamı' diye belirtip yazsak yetmez mi"); a clip cut from the middle loses the card but keeps the watermark and the metadata. A lawyer is to confirm this reading. The video tool does not add the watermark or the metadata yet; that is an open issue.

## 8. Kit (`kind: kit`)

A kit says how the components of `ui.yaml` look in one product: `tokens` and `dark` (design tokens files in the Design Tokens Format Module 2025.10 of the W3C Design Tokens Community Group), `css` (scoped to the screen), and per component a `template` with slots (`{text}`, `{name}`, `{intent}`, `{id}`, `{children}`), flags (`{?disabled}..{/disabled}`, `named`, `conditional`, `checked`: a node whose `checked` is `true`), `roles` and `intents`. A kit is data: nothing in it runs or is fetched. `kit check` proves that every node keeps the role, the name and the disabled state the graph expects.

## 9. Source anchors

`source: <path>:<line>` on a state, a transition, a guard, a node, an entity or an action names the code that makes it true, relative to the application's repository root. A bare `<path>` anchors a whole file. In capture mode every element carries one; in create mode an element carries one when the code already exists. `check` reports a state of a captured flow that names no code (`source-missing`). An exit may leave it out: the transition into it names the code, and an exit such as the person leaving the page has no code of its own.

`anchors.lock.yaml`, generated beside the graph, records what each anchor pointed at (the line, its neighbours' hashes, the commit). The check finds every line again and says whether it is unchanged, moved, changed, gone or ambiguous. A `file:line` in a description or a comment is not an anchor.

## 10. Checks and findings

| Exit | Meaning |
|---|---|
| 0 | the graph loads and, under `--strict`, nothing is wrong |
| 1 | the graph cannot be loaded: a YAML error, unsupported YAML (§1), an unknown state, an unknown name in an expression or one that does not parse, a non-exhaustive `always`, an unsupported construct, a `ui.yaml` that names no catalog or a catalog that is not there or not JSON (§4) |
| 2 | under `--strict`: an assertion is violated, a state no situation reaches or a state that is no exit and has no way out (§2), a catalog key is missing from a locale's catalog or a sample value from the samples, a component has no renderer, the kit does not cover a component, the main story cannot be walked, the focus goes to a node that is not shown or a `returnsFocus` does not start at a dialog (§4), a domain's files clash, a server function has what only a route has, a domain names an entity or a field it does not have, a policy names an action or an entity its domain does not have, or a flow invokes an action its domain does not have, names a domain that is not found, or lists a failure code that is neither a refusal of its action nor a transport code (§5), a tutorial no longer matches its flow (§7), a state of a captured flow names no code (§9), or a need's evidence does not resolve, a need that is not missing names none, or a product's gaps and its needs disagree (§13) |

A graph that cannot be loaded is a finding of the rule `load`, at its place: the file and the line of what it is about (the transition that goes to a state that does not exist, the guard that does not parse, the `catalog` of `ui.yaml`), or the line and column a parser gave. The other graphs are checked as usual.

**As text.** A finding is one line in the GNU error format, which compilers, editors and most CI logs read:

```
graphs/panel/nav/nav.yaml:42:5: error: the menu entry opens a page that refuses [menu-refused]
graphs/product/flow.yaml:88:7: note: step 2: CODE_CHANGED from developing, with replayed = false
```

`error` fails under `--strict` and `warning` does not; the `note` lines after a finding give its path or the places it relates to. A gap of a product (§13) is a finding at the level `note`, which fails nothing. With `--baseline`, an error the base run had is printed as a `warning`, as it fails nothing, but a `load` error fails the run however often it was seen and is printed as an `error`. The summary on the last line counts the findings at the levels they are printed at, and once a flow has needs, the gaps among them (§13). On GitHub a problem matcher turns these lines into annotations on the pull request, at no cost and on pull requests from forks too.

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
| a gap: a need that is missing and listed (§13) | a `result` of the rule `need-gap` at `level: note` |
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
| OpenAPI 3.1 | an action with a route is an operation (§5); a server function is not | export and import: #13 |
| RFC 9457, problem details | the body of a refusal (§5) | as it is |
| SCXML (W3C, 2015) | what a flow means (§2) | a subset; export: #14 |
| WAI-ARIA 1.2, Accessible Name and Description Computation 1.2 | roles, names and checked states in `ui.yaml` (§4) | as they are |
| ARIA Authoring Practices Guide (W3C WAI) | where the focus goes when a dialog opens and closes (§4) | its dialog pattern, as it is |
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

delurio's own, because no standard covers them: one graph whose flows, screens, pages and API are checked against each other; the checker and the guarantees it proves; the expression language (§3); navigation graphs (§6); bindings and tutorials (§7); source anchors (§9); needs and gaps (§13); the revision diff in words.

## 13. Needs and gaps

A flow draws what it does, and `world: open` (§2) leaves out the rest: what is not drawn is not ruled out. So nothing holds a product to what its users need: a feature that is missing is simply not drawn, and no check fails. A flow's needs are a closed list of what must exist for its users, each need with the evidence that it does, or with why it does not yet. The needs that are missing are the product's gaps, listed in one file. A product built in phases is so written whole, the phases to come included: its check passes while every gap is listed, and fails on a gap that is not, or on one that is met and still listed.

### A flow's needs (`kind: needs`)

```yaml
schema: delurio-graph/0
kind: needs
id: example.restoreNote
status: proposed
needs:
  - id: restore-from-its-page
    need: The owner of an archived note restores it from the note's own page, and it is active again.
    evidence:
      - state: restoring
      - node: restore
      - source: app/notes.js
        name: restoreNote
      - test: app/notes.test.js
        name: restores an archived note
      - scenario: owner
  - id: only-its-owner
    need: Only the note's owner reads and restores it; to anyone else it is no note at all.
    evidence:
      - source: app/notes.js:12
      - setting: NOTES_SESSION_SECRET
        in: .env.example
  - id: says-when-it-is-erased
    need: The page of an archived note says how long the note is kept before it is erased.
    evidence:
      - node: keptFor
    missing: archived notes are not erased yet; the second phase erases them
```

A flow's needs are in `needs.yaml`, in the flow's folder beside `flow.yaml`, and its `id` is the flow's. A need has:

| Key | Meaning |
|---|---|
| `id` | the need's id in its flow, kebab-case |
| `need` | what must exist, in a sentence of the graph's language, from the users' side: what they can do, or what they are told |
| `evidence` | what shows that it exists (below) |
| `missing` | that it does not exist yet, and why or where that is tracked: words, an issue, a finding's id |
| `note` | the author's own words, which no tool reads |

A need without `missing` is met, and its evidence names at least one thing outside the graph: a `source`, a `test`, a `scenario` or a `setting`. What the graph draws (`state`, `node`) says where the need is drawn, and alone shows nothing built. A need with `missing` is a gap. It may name evidence too: where it is drawn, and what exists of it so far. Needs are written from what the flow's users need, never from what the product does today, so a need the product does not meet yet is written all the same, with `missing`. A list is therefore `proposed` or `approved`, never `captured` (§1).

| Evidence | What it says | It resolves when |
|---|---|---|
| `source: <path>:<line>`, `source: <path>` | the code that makes the need true, an anchor as in §9 | the file exists and has that line; with `name`, the file holds the name |
| `test: <path>` | a test of the need | the file exists; with `name`, the file holds the test's name as it is written: in quotes, or a test function's name |
| `scenario: <scenario>` | a scenario of a binding (§7) that walks the need on the app | the binding exists, binds this flow, and has that scenario, its `extends` included; without `binding`, the binding is the flow's own `binding.yaml` |
| `state: <state>` | a state of the flow the need is drawn as | the flow has that state |
| `node: <node>` | a node of the flow's screens the need is drawn as | the flow's `ui.yaml` has that node |
| `setting: <NAME>` | an environment variable a deployment sets for the need | a line of the file `in` names sets it and is not commented out: `NAME=` or `export NAME=`, as a dotenv file has it, or `NAME:` or `- NAME=`, as the `environment` of a Compose file has it |

A `source` or a `test` may say what its file holds (`name`), and a `scenario` which binding it is in (`binding: <path>`); a `setting` names the file that sets it (`in: <path>`). A file holds a name when the name is in it and is not part of a longer word. Every path is relative to the application's repository root, as a `source:` anchor's is (§9): the folder `check --app` names, or else the folder `check` runs in, as a CI runs it at the root of the repository it checked out. A piece of evidence is of one kind; a need names as many as it has.

Evidence shows that something exists, not that it is right or that it passes. A test is run, and a scenario replayed, by the CI that runs them. Whoever writes and approves the list names only a test or a scenario that walks the need as its users meet it.

### A product's gaps (`kind: gaps`)

```yaml
schema: delurio-graph/0
kind: gaps
id: example.notes
gaps:
  example.restoreNote: [says-when-it-is-erased]
```

`needs.gaps.yaml` lists every need of a product's flows that says `missing`: under each flow's id, the ids of its needs. Its `id` is the product's. It sits at the root of the product's graphs, `graphs/needs.gaps.yaml`, and covers the flows under its folder: a flow's gaps are listed in the nearest `needs.gaps.yaml` in its folder or above it, up to the root of its repository (the first folder that holds `.git`).

### What the tools do with them

`check` reads every `needs.yaml` and `needs.gaps.yaml` under the folders it is given, validates each against its schema (`schema`), and holds them to each other and to what they name:

- `evidence-unresolved`, at a piece of evidence that does not resolve: a file that is not there, a line past the file's end, a name the file does not hold, a binding that is not there or binds another flow, a scenario it does not have, a state or a node the flow does not have, a setting no line of its file sets. So evidence that goes away takes its need with it.
- `need-no-evidence`, at a need that is not missing and whose evidence names only what the graph draws.
- `need-unlisted`, at a need that is missing and that its gaps file does not list, or that has no gaps file above it.
- `gap-met`, at an entry of a gaps file whose need is met now; `gap-unknown`, at an entry that names a flow or a need that no `needs.yaml` under the file's folder has.
- `need-twice`, at a need whose id is given earlier in its file; `needs-place`, at a `needs.yaml` that is beside no flow, or whose `id` is not the id of the flow beside it.

Each is an error, and fails `--strict` (exit 2). A gap that is listed fails nothing: it is a finding of its own, `need-gap`, at the need's `missing`, with its words, at the level `note` in text and in SARIF. Once a flow has needs, the summary line counts the gaps among them:

```
examples/notes/restore/needs.yaml:47:5: note: need says-when-it-is-erased is missing: archived notes are not erased yet; … [need-gap]
delurio check: 5 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0
```

So the gaps only shrink on their own. A need that is met leaves the list, or `gap-met` fails; a need cannot become a gap unless the list says so, or `need-unlisted` fails. Each new gap is therefore a line added to `needs.gaps.yaml`, which the review of that change reads. With `--baseline` (§10), a gap the base run did not have is `new` in SARIF, and the summary says how many are new: `2 gaps of 4 needs (1 new)`.

A tool that walks a flow on the app leaves out what only missing needs are drawn as, rather than failing on it: the states and the nodes that a need with `missing` names as its evidence, and that no met need names. A replay (§7) takes no step into such a state: a walk that would ends before it, and `--cover` keeps no path to it. It does not look for such a node, and it lists what it left out with the needs' ids. A tutorial (§7) whose steps show such a state or node is not made into a video until those needs are met. `check` replays a tutorial on the flow itself, where the planned parts are drawn, and holds it as before. `planned` in `@delurio/cli/needs` gives the planned states and nodes of a flow, each with the needs it waits on.

A graph with no `needs.yaml` is checked as before. Decided on 15 Nov 2026.
