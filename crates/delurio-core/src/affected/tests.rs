//! The selection of the graphs a change touches, on a customer's repository made here: a shop whose graphs were
//! captured from its code, so every element carries a `source:` anchor (SPEC.md §9), with a shared domain, a
//! navigation graph, a kit and a catalog that two flows read, and bindings outside the graphs folder with a base, a
//! helper and a fixture. Each case changes the repository and reads what the selection says, and why. The change is
//! given as `git diff -U0` would give it: the paths in order, each with its one hunk.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

fn lines(n: usize, name: &str) -> String {
    (1..=n).map(|i| format!("// {name} line {i}\n")).collect()
}

const FILES: [(&str, &str); 16] = [
    (
        "graphs/shop/pay/flow.yaml",
        "schema: delurio-graph/0
kind: flow
id: shop.pay
status: captured
domain: shop.api
ui: shop.pay
title: Pay for the order
initial: form
states:
  form:
    title: Fill the card
    source: src/pay.ts:3
    on:
      PAY: { target: paying, source: src/pay.ts:8 }
  paying:
    title: Pay
    source: src/pay.ts:12
    invoke: { action: pay }
    on:
      DONE: { target: paid, source: src/pay.ts:14 }
  paid: { type: final, title: Paid, source: src/pay.ts:16 }
",
    ),
    (
        "graphs/shop/pay/ui.yaml",
        "schema: delurio-graph/0
kind: ui
id: shop.pay
flow: shop.pay
status: captured
catalog: ../shared/catalog/en.json
locales: [en, tr]
kit: ../kit
nodes:
  payButton: { type: action, component: Button, role: button, text: { key: Pay.go }, in: [form], emits: PAY, source: src/PayForm.tsx:5 }
",
    ),
    (
        "graphs/shop/pay/binding.yaml",
        "schema: delurio-graph/0
kind: binding
flow: shop.pay
apps: { web: \"https://shop.example\" }
",
    ),
    ("graphs/shop/pay/viz/flow.mmd", "flowchart TD\n"),
    (
        "graphs/shop/refund/flow.yaml",
        "schema: delurio-graph/0
kind: flow
id: shop.refund
status: captured
domain: shop.api
title: Refund an order
initial: asking
states:
  asking:
    title: Ask for a refund
    source: src/refund.ts
    invoke: { action: refund }
    on:
      DONE: { target: refunded, source: src/refund.ts:4 }
  refunded: { type: final, title: Refunded, source: src/refund.ts:6 }
",
    ),
    (
        "graphs/shop/refund/ui.yaml",
        "schema: delurio-graph/0
kind: ui
id: shop.refund
flow: shop.refund
status: captured
catalog: ../shared/catalog/en.json
nodes: {}
",
    ),
    (
        "graphs/shop/browse/flow.yaml",
        "schema: delurio-graph/0
kind: flow
id: shop.browse
status: captured
domain: shop.api
title: Browse the shop
initial: listing
states:
  listing: { title: List, source: src/browse.ts:2, invoke: { action: listProducts }, on: { DONE: { target: listed, source: src/browse.ts:3 } } }
  listed: { type: final, title: Listed, source: src/browse.ts:4 }
",
    ),
    ("graphs/shop/shared/catalog/en.json", "{ \"Pay.go\": \"Pay\" }\n"),
    ("graphs/shop/shared/catalog/tr.json", "{ \"Pay.go\": \"Öde\" }\n"),
    ("graphs/shop/kit/kit.yaml", "schema: delurio-graph/0\nkind: kit\nid: shop.kit\n"),
    ("graphs/shop/kit/tokens.json", "{}\n"),
    (
        "graphs/shop/api/domain.yaml",
        "schema: delurio-graph/0
kind: domain
id: shop.api
status: captured
entities:
  Order:
    source: src/db/order.ts:1
    fields:
      id: { type: string }
      total: { type: number, source: src/db/order.ts:3 }
  Product:
    fields:
      id: { type: string }
actions:
  pay:
    source: src/api/pay.ts:10
    result: { returns: Order }
  refund:
    source: src/api/refund.ts:2
    body: { type: object, properties: { order: { type: string, references: Order } } }
  listProducts:
    source: src/api/products.ts:1
    result: { returns: Product }
policies:
  - { actor: person, needs: session, write: [Order], do: [pay] }
invariants:
  - { id: refunds-are-whole, rule: \"refund gives back the whole total\" }
",
    ),
    (
        "graphs/shop/nav/nav.yaml",
        "schema: delurio-graph/0
kind: nav
id: shop.nav
status: captured
catalog: catalog/en.json
source: { app: src }
pages:
  home: { route: /, source: src/pages/home.tsx:2 }
  cart: { route: /cart, source: src/pages/cart.tsx }
entries:
  - { from: home, to: cart, how: link, label: Nav.cart, source: src/pages/home.tsx:4 }
",
    ),
    ("graphs/shop/nav/catalog/en.json", "{ \"Nav.cart\": \"Cart\" }\n"),
    ("graphs/README.md", "# The graphs\n"),
    (
        "e2e/bindings/base.binding.yaml",
        "schema: delurio-graph/0
kind: binding
root: ..
apps: { web: \"http://localhost:3000\" }
data: { people: { json: fixtures/people.json } }
",
    ),
];

const BINDINGS: [(&str, &str); 2] = [
    (
        "e2e/bindings/refund.binding.yaml",
        "schema: delurio-graph/0
kind: binding
extends: base.binding.yaml
flow: shop.refund
scenarios:
  main:
    given: {}
    setup:
      - run: [node, seed.ts, --orders, \"3\"]
    enter: { nav: ../../graphs/shop/nav, bind: nav.binding.yaml, entry: home->cart }
",
    ),
    (
        "e2e/bindings/nav.binding.yaml",
        "schema: delurio-graph/0
kind: binding
nav: shop.nav
apps: { web: \"http://localhost:3000\" }
",
    ),
];

/// the shop's files
fn shop() -> BTreeMap<String, String> {
    let mut files: BTreeMap<String, String> =
        FILES.iter().chain(&BINDINGS).map(|(p, t)| (p.to_string(), t.to_string())).collect();
    for (path, n, name) in [
        ("e2e/bindings/checks.ts", 5, "checks"),
        ("e2e/seed.ts", 5, "seed"),
        ("e2e/other/notes.ts", 3, "notes"),
        ("src/pay.ts", 40, "pay"),
        ("src/PayForm.tsx", 20, "form"),
        ("src/refund.ts", 10, "refund"),
        ("src/browse.ts", 10, "browse"),
        ("src/api/pay.ts", 30, "api pay"),
        ("src/api/refund.ts", 10, "api refund"),
        ("src/api/products.ts", 10, "api products"),
        ("src/db/order.ts", 10, "order"),
        ("src/pages/home.tsx", 10, "home"),
        ("src/pages/cart.tsx", 10, "cart"),
        ("src/util.ts", 10, "util"),
    ] {
        files.insert(path.to_string(), lines(n, name));
    }
    files.insert("e2e/fixtures/people.json".into(), "[]\n".into());
    files.insert("README.md".into(), "# Shop\n".into());
    files
}

/// the `-U0` hunk of a change that is one run of lines: the lines both sides start and end with are kept
fn hunks_of(before: &str, after: &str) -> Vec<Hunk> {
    let a: Vec<&str> = before.split_inclusive('\n').collect();
    let b: Vec<&str> = after.split_inclusive('\n').collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (old_lines, new_lines) = (a.len() - head - tail, b.len() - head - tail);
    let start = |n: usize| (if n == 0 { head } else { head + 1 }) as u64;
    vec![Hunk {
        old_start: start(old_lines),
        old_lines: old_lines as u64,
        new_start: start(new_lines),
        new_lines: new_lines as u64,
    }]
}

/// the files that differ between two sides, in path order, as `git diff --name-status` lists them
fn diff(base: &BTreeMap<String, String>, head: &BTreeMap<String, String>) -> Vec<ChangedFile> {
    let paths: BTreeSet<&String> = base.keys().chain(head.keys()).collect();
    paths
        .into_iter()
        .filter_map(|p| {
            let (status, a, b) = match (base.get(p), head.get(p)) {
                (Some(a), Some(b)) if a == b => return None,
                (Some(a), Some(b)) => (ChangeStatus::Modified, a.as_str(), b.as_str()),
                (None, Some(b)) => (ChangeStatus::Added, "", b.as_str()),
                (Some(a), None) => (ChangeStatus::Deleted, a.as_str(), ""),
                (None, None) => return None,
            };
            Some(ChangedFile { path: p.clone(), from: None, status, hunks: Some(hunks_of(a, b)) })
        })
        .collect()
}

/// the shop with `edit` made to it; the selection of that change
fn changed(edit: impl FnOnce(&mut BTreeMap<String, String>)) -> Affected {
    let base = shop();
    let mut head = base.clone();
    edit(&mut head);
    let files = diff(&base, &head);
    let (h, b) = (MapTree::new(head), MapTree::new(base));
    affected(&Change::new("graphs", &h, Some(&b), Some(files)))
}

/// rewrites line `line` (1-based) of a file
fn edit_line(files: &mut BTreeMap<String, String>, path: &str, line: usize, text: &str) {
    let file = files.get_mut(path).expect("the file is there");
    let mut all: Vec<&str> = file.split('\n').collect();
    all[line - 1] = text;
    *file = all.join("\n");
}

fn put(files: &mut BTreeMap<String, String>, path: &str, text: &str) {
    files.insert(path.to_string(), text.to_string());
}

fn replace(files: &mut BTreeMap<String, String>, path: &str, from: &str, to: &str) {
    let file = files.get_mut(path).expect("the file is there");
    assert!(file.contains(from), "{path} has {from}");
    *file = file.replace(from, to);
}

fn ids(a: &Affected) -> Vec<&str> {
    a.graphs.iter().map(|g| g.id.as_str()).collect()
}

fn why(a: &Affected, id: &str) -> Vec<String> {
    let g = a.graphs.iter().find(|g| g.id == id);
    g.map(|g| &g.reasons)
        .into_iter()
        .flatten()
        .map(|r| match r.precision {
            None => format!("{}: {}", r.rule.as_str(), r.say),
            Some(p) => format!("{}/{}: {}", r.rule.as_str(), p.as_str(), r.say),
        })
        .collect()
}

#[test]
fn a_changed_line_an_anchor_names_touches_its_flow_line_precise_and_a_line_no_anchor_names_touches_nothing() {
    let a = changed(|f| edit_line(f, "src/pay.ts", 12, "// changed"));
    assert_eq!(a.all, None);
    assert_eq!(ids(&a), ["shop.pay"]);
    assert_eq!(
        why(&a, "shop.pay"),
        ["anchor/line: line 12 of src/pay.ts changed: the anchor of states.paying in flow.yaml"]
    );
    let b = changed(|f| edit_line(f, "src/pay.ts", 30, "// changed"));
    assert_eq!(ids(&b), Vec::<&str>::new());
    assert_eq!(b.unmapped, ["src/pay.ts"]);
}

#[test]
fn a_line_inserted_next_to_an_anchored_line_touches_it_and_a_node_anchor_touches_the_flow_its_screen_is() {
    let a = changed(|f| {
        let file = f.get_mut("src/PayForm.tsx").expect("the form is there");
        let mut all: Vec<&str> = file.split('\n').collect();
        all.insert(5, "// a line after the anchored one");
        *file = all.join("\n");
    });
    assert_eq!(ids(&a), ["shop.pay"]);
    assert_eq!(
        why(&a, "shop.pay"),
        ["anchor/line: line 5 of src/PayForm.tsx changed: the anchor of nodes.payButton in ui.yaml"]
    );
}

#[test]
fn an_anchor_on_a_whole_file_is_held_at_file_level_and_says_so() {
    let a = changed(|f| edit_line(f, "src/refund.ts", 9, "// changed"));
    assert_eq!(ids(&a), ["shop.refund"]);
    assert_eq!(
        why(&a, "shop.refund"),
        [
            "anchor/file: src/refund.ts changed, and the anchor names the whole file: the anchor of states.asking in flow.yaml"
        ]
    );
}

#[test]
fn a_domain_change_touches_only_the_flows_invoking_the_changed_actions() {
    let a = changed(|f| {
        replace(
            f,
            "graphs/shop/api/domain.yaml",
            "    source: src/api/refund.ts:2\n",
            "    source: src/api/refund.ts:2\n    description: now in two steps\n",
        )
    });
    assert_eq!(ids(&a), ["shop.refund"]);
    assert_eq!(
        why(&a, "shop.refund"),
        ["domain: graphs/shop/api/domain.yaml changes the action refund, which it invokes"]
    );
}

#[test]
fn a_changed_entity_touches_the_flows_whose_actions_name_it_and_a_domain_anchor_the_flows_of_its_action() {
    let a = changed(|f| {
        replace(f, "graphs/shop/api/domain.yaml", "      total: { type: number", "      total: { type: integer")
    });
    assert_eq!(ids(&a), ["shop.pay", "shop.refund"]);
    assert_eq!(
        why(&a, "shop.pay"),
        ["domain: graphs/shop/api/domain.yaml changes the entity Order, which the action pay names, which it invokes"]
    );
    let b = changed(|f| {
        edit_line(f, "src/api/pay.ts", 10, "// changed");
        edit_line(f, "src/db/order.ts", 9, "// changed");
    });
    assert_eq!(ids(&b), ["shop.pay"]);
    assert_eq!(
        why(&b, "shop.pay"),
        ["anchor/line: line 10 of src/api/pay.ts changed: the anchor of actions.pay in shop.api, which it invokes"]
    );
    assert_eq!(b.unmapped, ["src/db/order.ts"]);
}

#[test]
fn a_changed_policy_or_invariant_touches_the_flows_of_the_actions_it_names() {
    let a = changed(|f| {
        replace(f, "graphs/shop/api/domain.yaml", "needs: session, write: [Order]", "needs: admin, write: [Order]")
    });
    assert_eq!(ids(&a), ["shop.pay"]);
    assert_eq!(
        why(&a, "shop.pay"),
        ["domain: graphs/shop/api/domain.yaml changes a policy that lists pay, which it invokes"]
    );
    let b = changed(|f| replace(f, "graphs/shop/api/domain.yaml", "the whole total\"", "the whole total, once\""));
    assert_eq!(ids(&b), ["shop.refund"]);
    assert_eq!(
        why(&b, "shop.refund"),
        [
            "domain: graphs/shop/api/domain.yaml changes the invariant refunds-are-whole, which names refund, which it invokes"
        ]
    );
    // a key of the domain that no action can be told apart by touches every flow of it
    let c = changed(|f| {
        replace(
            f,
            "graphs/shop/api/domain.yaml",
            "status: captured\nentities:",
            "status: captured\nservice: shop\nentities:",
        )
    });
    assert_eq!(ids(&c), ["shop.browse", "shop.pay", "shop.refund"]);
    assert_eq!(why(&c, "shop.browse"), ["domain: the domain's service changed: every flow of shop.api"]);
}

#[test]
fn a_domain_split_across_files_is_read_as_one_and_an_anchor_on_a_policy_touches_the_flows_of_its_actions() {
    let split = |f: &mut BTreeMap<String, String>| {
        put(
            f,
            "graphs/shop/api/rules.yaml",
            "schema: delurio-graph/0\nkind: domain\nid: shop.api\npolicies:\n  - { actor: person, needs: session, do: [refund], source: src/api/rules.ts:2 }\n",
        );
        put(f, "src/api/rules.ts", &lines(5, "rules"));
    };
    let base = {
        let mut files = shop();
        split(&mut files);
        files
    };
    let mut head = base.clone();
    edit_line(&mut head, "src/api/rules.ts", 2, "// changed");
    let files = diff(&base, &head);
    let (h, b) = (MapTree::new(head), MapTree::new(base));
    let a = affected(&Change::new("graphs", &h, Some(&b), Some(files)));
    assert_eq!(ids(&a), ["shop.refund"]);
    assert_eq!(
        why(&a, "shop.refund"),
        ["anchor/line: line 2 of src/api/rules.ts changed: the anchor of policies.0 in shop.api, which it invokes"]
    );
    // a new part of the domain changes what it says, as the other part does
    let c = changed(split);
    assert_eq!(ids(&c), ["shop.refund"]);
    assert_eq!(
        why(&c, "shop.refund"),
        [
            "domain: graphs/shop/api/rules.yaml changes a policy that lists refund, which it invokes",
            "anchor/file: src/api/rules.ts is added: the anchor of policies.0 in shop.api, which it invokes",
        ]
    );
    assert_eq!(c.unmapped, Vec::<String>::new());
}

#[test]
fn the_graph_files_of_a_flow_touch_it_and_generated_files_and_words_for_people_touch_nothing() {
    let a = changed(|f| {
        edit_line(f, "graphs/shop/browse/flow.yaml", 6, "title: Browse the whole shop");
        put(f, "graphs/shop/pay/viz/flow.mmd", "flowchart LR\n");
        put(f, "graphs/README.md", "# The graphs, again\n");
        put(f, "graphs/shop/refund/anchors.lock.yaml", "schema: delurio-graph/0\n");
    });
    assert_eq!(ids(&a), ["shop.browse"]);
    assert_eq!(why(&a, "shop.browse"), ["graph: its own file flow.yaml is changed"]);
    assert_eq!(
        a.unmapped,
        ["graphs/README.md", "graphs/shop/pay/viz/flow.mmd", "graphs/shop/refund/anchors.lock.yaml"]
    );
}

#[test]
fn a_shared_catalog_and_a_kit_touch_the_graphs_that_name_them() {
    let a = changed(|f| put(f, "graphs/shop/shared/catalog/tr.json", "{ \"Pay.go\": \"Ödeme yap\" }\n"));
    assert_eq!(ids(&a), ["shop.pay"]);
    assert_eq!(why(&a, "shop.pay"), ["shared: graphs/shop/shared/catalog/tr.json is changed: a catalog it names"]);
    let b = changed(|f| {
        put(f, "graphs/shop/shared/catalog/en.json", "{ \"Pay.go\": \"Pay now\" }\n");
        put(f, "graphs/shop/kit/tokens.json", "{ \"a\": 1 }\n");
    });
    assert_eq!(ids(&b), ["shop.pay", "shop.refund"]);
    assert!(why(&b, "shop.pay").contains(&"shared: graphs/shop/kit/tokens.json is changed: its kit".to_string()));
}

#[test]
fn a_base_binding_touches_the_bindings_that_extend_it_and_a_helper_a_fixture_or_a_file_beside_it_its_flow() {
    let base = changed(|f| replace(f, "e2e/bindings/base.binding.yaml", "3000", "3001"));
    assert_eq!(ids(&base), ["shop.refund"]);
    assert_eq!(
        why(&base, "shop.refund"),
        [
            "binding: its binding e2e/bindings/refund.binding.yaml extends e2e/bindings/base.binding.yaml, which is changed"
        ]
    );
    let helper = changed(|f| {
        edit_line(f, "e2e/seed.ts", 2, "// changed");
        put(f, "e2e/fixtures/people.json", "[{}]\n");
    });
    assert_eq!(ids(&helper), ["shop.refund"]);
    assert_eq!(
        why(&helper, "shop.refund"),
        [
            "helper: its binding e2e/bindings/base.binding.yaml names e2e/fixtures/people.json",
            "helper: its binding e2e/bindings/refund.binding.yaml names e2e/seed.ts",
        ]
    );
    let place = changed(|f| put(f, "e2e/package.json", "{}\n"));
    assert_eq!(
        why(&place, "shop.refund"),
        [
            "helper: e2e/package.json sits in e2e, where its binding e2e/bindings/base.binding.yaml runs its commands",
            "helper: e2e/package.json sits in e2e, where its binding e2e/bindings/refund.binding.yaml runs its commands",
        ]
    );
    let beside = changed(|f| edit_line(f, "e2e/bindings/checks.ts", 1, "// changed"));
    assert_eq!(ids(&beside), ["shop.nav", "shop.refund"]);
    let elsewhere = changed(|f| edit_line(f, "e2e/other/notes.ts", 1, "// changed"));
    assert_eq!(ids(&elsewhere), Vec::<&str>::new());
    assert_eq!(elsewhere.unmapped, ["e2e/other/notes.ts"]);
}

#[test]
fn a_variant_of_a_binding_is_a_binding_beside_its_flow_and_elsewhere() {
    let beside = changed(|f| {
        put(f, "graphs/shop/pay/binding.noCard.yaml", "schema: delurio-graph/0\nkind: binding\nflow: shop.pay\n")
    });
    assert_eq!(ids(&beside), ["shop.pay"]);
    assert_eq!(why(&beside, "shop.pay"), ["graph: its binding binding.noCard.yaml is added"]);
    assert_eq!(beside.graphs[0].bindings, ["graphs/shop/pay/binding.noCard.yaml", "graphs/shop/pay/binding.yaml"]);
    let elsewhere = changed(|f| {
        put(
            f,
            "e2e/bindings/binding.slow.yaml",
            "schema: delurio-graph/0\nkind: binding\nextends: base.binding.yaml\nflow: shop.browse\n",
        )
    });
    assert_eq!(ids(&elsewhere), ["shop.browse"]);
    assert_eq!(why(&elsewhere, "shop.browse"), ["binding: its binding e2e/bindings/binding.slow.yaml is added"]);
}

#[test]
fn a_binding_that_enters_through_the_navigation_graph_is_touched_when_the_graphs_files_change() {
    let a = changed(|f| replace(f, "graphs/shop/nav/nav.yaml", "route: /cart", "route: /basket"));
    assert_eq!(ids(&a), ["shop.nav", "shop.refund"]);
    assert_eq!(
        why(&a, "shop.refund"),
        ["entered: its binding e2e/bindings/refund.binding.yaml enters through graphs/shop/nav, whose files changed"]
    );
    assert_eq!(a.graphs[0].bindings, ["e2e/bindings/nav.binding.yaml"]);
    assert_eq!(a.graphs[1].bindings, ["e2e/bindings/refund.binding.yaml"]);
}

// A node that draws another flow's screen (SPEC.md §7, `draws`), such as a guide that draws the app's form in a
// dialog of its own, is checked against that flow's nodes: a change to the drawn flow's files touches the binding's
// flow, and so does a change to the binding `draws` names. Proposed on 23 Nov 2026.
#[test]
fn a_binding_whose_node_draws_a_flow_is_touched_when_that_flows_files_change() {
    let drawing = "schema: delurio-graph/0
kind: binding
extends: base.binding.yaml
flow: shop.browse
nodes:
  payPart: { role: dialog, name: Pay, draws: { flow: ../../graphs/shop/pay, bind: ../../graphs/shop/pay/binding.yaml } }
";
    let mut base = shop();
    put(&mut base, "e2e/bindings/browse.binding.yaml", drawing);
    let run = |edit: &dyn Fn(&mut BTreeMap<String, String>)| {
        let mut head = base.clone();
        edit(&mut head);
        let files = diff(&base, &head);
        let (h, b) = (MapTree::new(head), MapTree::new(base.clone()));
        affected(&Change::new("graphs", &h, Some(&b), Some(files)))
    };
    let ui = run(&|f| replace(f, "graphs/shop/pay/ui.yaml", "src/PayForm.tsx:5", "src/PayForm.tsx:4"));
    assert_eq!(ids(&ui), ["shop.browse", "shop.pay"]);
    assert_eq!(
        why(&ui, "shop.browse"),
        ["drawn: its binding e2e/bindings/browse.binding.yaml draws graphs/shop/pay, whose files changed"]
    );
    let bind = run(&|f| replace(f, "graphs/shop/pay/binding.yaml", "flow: shop.pay", "flow: shop.pay\nmask: []"));
    assert_eq!(
        why(&bind, "shop.browse"),
        [
            "drawn: its binding e2e/bindings/browse.binding.yaml draws graphs/shop/pay, whose files changed",
            "drawn: its binding e2e/bindings/browse.binding.yaml draws graphs/shop/pay/binding.yaml, whose files changed",
        ]
    );
}

#[test]
fn a_renamed_or_deleted_file_an_anchor_names_is_held_at_file_level() {
    let base = shop();
    let (h, b) = (MapTree::new(base.clone()), MapTree::new(base));
    let renamed = ChangedFile {
        path: "src/payment.ts".into(),
        from: Some("src/refund.ts".into()),
        status: ChangeStatus::Renamed,
        hunks: None,
    };
    let a = affected(&Change::new("graphs", &h, Some(&b), Some(vec![renamed])));
    assert_eq!(
        why(&a, "shop.refund"),
        [
            "anchor/file: src/refund.ts is renamed: the anchor of states.asking in flow.yaml",
            "anchor/file: src/refund.ts is renamed: the anchor of states.refunded in flow.yaml",
            "anchor/file: src/refund.ts is renamed: the anchor of states.asking.on.DONE in flow.yaml",
        ]
    );
    let gone = changed(|f| {
        f.remove("src/browse.ts");
    });
    assert!(why(&gone, "shop.browse").iter().all(|w| w.starts_with("anchor/file: src/browse.ts is deleted: ")));
}

#[test]
fn a_file_of_a_graph_that_is_gone_touches_nothing() {
    let a = changed(|f| {
        f.remove("graphs/shop/browse/flow.yaml");
    });
    assert_eq!(a.all, None);
    assert_eq!(ids(&a), Vec::<&str>::new());
    assert_eq!(a.unmapped, ["graphs/shop/browse/flow.yaml"]);
    assert_eq!(a.untouched, 3);
}

#[test]
fn anything_unknown_touches_every_graph_and_says_why() {
    let stray = changed(|f| put(f, "graphs/shop/stray.yaml", "a: 1\n"));
    assert_eq!(stray.all.as_deref(), Some("graphs/shop/stray.yaml is in the graphs folder and belongs to no graph"));
    assert_eq!(ids(&stray), ["shop.browse", "shop.nav", "shop.pay", "shop.refund"]);
    assert_eq!(stray.graphs[0].reasons[0].rule, Rule::All);
    assert_eq!(stray.unmapped, Vec::<String>::new());
    let broken = changed(|f| put(f, "graphs/shop/browse/flow.yaml", "a: [1\n"));
    let all = broken.all.unwrap_or_default();
    assert!(all.starts_with("graphs/shop/browse/flow.yaml does not load ("), "{all}");
    assert!(all.ends_with("), so what it names is not known"), "{all}");
    let not_a_map = changed(|f| put(f, "graphs/shop/browse/flow.yaml", "- a\n"));
    assert_eq!(
        not_a_map.all.as_deref(),
        Some("graphs/shop/browse/flow.yaml does not load (it is not a map), so what it names is not known")
    );
    let tree = MapTree::new(shop());
    let mut asked = Change::new("graphs", &tree, Some(&tree), Some(Vec::new()));
    asked.everything = Some("every graph was asked for (--all)".into());
    let asked = affected(&asked);
    assert_eq!(asked.all.as_deref(), Some("every graph was asked for (--all)"));
    assert_eq!(asked.graphs.len(), 4);
}

#[test]
fn given_as_paths_alone_anchors_hold_at_file_level_and_say_the_lines_are_not_known() {
    let graphs: Vec<(String, String)> = shop().into_iter().filter(|(p, _)| p.starts_with("graphs/")).collect();
    let tree = MapTree::new(graphs);
    let pay = ChangedFile { path: "src/pay.ts".into(), from: None, status: ChangeStatus::Modified, hunks: None };
    let a = affected(&Change::new("graphs", &tree, Some(&tree), Some(vec![pay.clone()])));
    assert_eq!(ids(&a), ["shop.pay"]);
    let said = why(&a, "shop.pay");
    assert_eq!(said.len(), 5);
    assert!(
        said.iter()
            .all(|w| w.starts_with("anchor/file: src/pay.ts changed; its changed lines are not known, so line "))
    );
    let mut short = Change::new("graphs", &tree, Some(&tree), Some(vec![pay]));
    short.complete = false;
    short.why = Some("the host lists 3000 at most".into());
    assert_eq!(
        affected(&short).all.as_deref(),
        Some(
            "the list of changed files was cut short (the host lists 3000 at most), so what else changed is not known"
        )
    );
    let mut unread = Change::new("graphs", &tree, None, None);
    unread.why = Some("the host did not answer".into());
    let unread = affected(&unread);
    assert_eq!(unread.all.as_deref(), Some("the change cannot be read: the host did not answer"));
    assert_eq!(unread.changed, 0);
}

#[test]
fn a_caller_without_a_checkout_selects_on_the_graphs_texts_the_texts_before_and_the_hosts_paths() {
    // a revision's graphs, the approved copy's text of the file it changes, and the paths a host lists
    let before: Vec<(String, String)> = shop().into_iter().filter(|(p, _)| p.starts_with("graphs/")).collect();
    let mut after: BTreeMap<String, String> = before.iter().cloned().collect();
    replace(
        &mut after,
        "graphs/shop/api/domain.yaml",
        "    source: src/api/refund.ts:2\n",
        "    source: src/api/refund.ts:2\n    description: two steps\n",
    );
    let (head, base) = (MapTree::new(after), MapTree::new(before.clone()));
    let paths = ["graphs/shop/api/domain.yaml", "src/browse.ts", "docs/how.md"];
    let files: Vec<ChangedFile> = paths
        .iter()
        .map(|p| ChangedFile { path: p.to_string(), from: None, status: ChangeStatus::Modified, hunks: None })
        .collect();
    let a = affected(&Change::new("graphs", &head, Some(&base), Some(files)));
    assert_eq!(a.all, None);
    assert_eq!(ids(&a), ["shop.browse", "shop.refund"]);
    assert_eq!(
        why(&a, "shop.refund"),
        ["domain: graphs/shop/api/domain.yaml changes the action refund, which it invokes"]
    );
    assert_eq!(
        why(&a, "shop.browse")[0],
        "anchor/file: src/browse.ts changed; its changed lines are not known, so line 2 is taken as changed: the anchor of states.listing in flow.yaml"
    );
    assert_eq!(a.unmapped, ["docs/how.md"]);

    // the approved copy had another title; the revision's own file and an anchored file of another flow
    let mut approved: BTreeMap<String, String> = before.iter().cloned().collect();
    replace(&mut approved, "graphs/shop/browse/flow.yaml", "Browse the shop", "Browse");
    let (head, base) = (MapTree::new(before), MapTree::new(approved));
    let files: Vec<ChangedFile> = ["graphs/shop/browse/flow.yaml", "src/PayForm.tsx", "package.json"]
        .iter()
        .map(|p| ChangedFile { path: p.to_string(), from: None, status: ChangeStatus::Modified, hunks: None })
        .collect();
    let a = affected(&Change::new("graphs/", &head, Some(&base), Some(files)));
    assert_eq!(ids(&a), ["shop.browse", "shop.pay"]);
    assert_eq!(why(&a, "shop.browse"), ["graph: its own file flow.yaml is changed"]);
    assert_eq!(
        why(&a, "shop.pay"),
        [
            "anchor/file: src/PayForm.tsx changed; its changed lines are not known, so line 5 is taken as changed: the anchor of nodes.payButton in ui.yaml"
        ]
    );
    assert_eq!(a.unmapped, ["package.json"]);
}

#[test]
fn the_answer_in_json_and_in_words() {
    let base = shop();
    let mut head = base.clone();
    edit_line(&mut head, "src/pay.ts", 3, "// changed");
    head.insert("docs/how.md".into(), "# How\n".into());
    let files = diff(&base, &head);
    let (h, b) = (MapTree::new(head), MapTree::new(base));
    let mut change = Change::new("graphs", &h, Some(&b), Some(files));
    change.label = Some(Label { base: Some("0123456789abcdef0123".into()), head: "fedcba9876543210".into() });
    let a = affected(&change);
    assert_eq!(
        a.to_text(),
        "1 graph touched since 0123456789ab by 2 changed files; 3 not touched
  graphs/shop/pay (shop.pay)
    - line 3 of src/pay.ts changed: the anchor of states.form in flow.yaml [line-precise]
touching no graph: docs/how.md
"
    );
    assert_eq!(
        a.to_json(),
        r#"{
  "schema": "delurio-affected/0",
  "base": "0123456789abcdef0123",
  "head": "fedcba9876543210",
  "all": null,
  "graphs": [
    {
      "id": "shop.pay",
      "dir": "graphs/shop/pay",
      "kind": "flow",
      "reasons": [
        {
          "rule": "anchor",
          "file": "src/pay.ts",
          "precision": "line",
          "say": "line 3 of src/pay.ts changed: the anchor of states.form in flow.yaml"
        }
      ],
      "bindings": [
        "graphs/shop/pay/binding.yaml"
      ]
    }
  ],
  "untouched": 3,
  "unmapped": [
    "docs/how.md"
  ],
  "changed": 2
}
"#
    );
    let nav = changed(|f| edit_line(f, "src/pages/home.tsx", 4, "// changed"));
    assert_eq!(
        nav.to_text(),
        "1 graph touched by 1 changed file; 3 not touched
  graphs/shop/nav (shop.nav)
    - line 4 of src/pages/home.tsx changed: the anchor of entries.0 in nav.yaml [line-precise]
"
    );
    let tree = MapTree::new(shop());
    let mut all = Change::new("graphs", &tree, None, None);
    all.why = Some("no-such-branch is not a commit of /x".into());
    assert_eq!(
        affected(&all).to_text(),
        "every graph (4): the change cannot be read: no-such-branch is not a commit of /x
  graphs/shop/browse (shop.browse)
  graphs/shop/nav (shop.nav)
  graphs/shop/pay (shop.pay)
  graphs/shop/refund (shop.refund)
"
    );
    // a graph no binding binds says so
    let browse = changed(|f| edit_line(f, "src/browse.ts", 4, "// changed"));
    assert!(browse.to_text().contains("  graphs/shop/browse (shop.browse), not bound\n"));
}

#[test]
fn the_anchors_are_where_the_schemas_give_them() {
    // every place a `source` is an anchor, as the kinds' schemas give them (SPEC.md §9), in the order of their places;
    // each place `ANCHOR_ORDER` lists is one of them
    let at = |kind: SchemaKind, text: &str| -> Vec<String> {
        let doc = read_yaml(text).expect("the file reads");
        let anchors = anchors_of(kind, "f.yaml", &doc);
        let known = ANCHOR_ORDER.iter().find(|(k, _)| *k == kind).map_or(0, |(_, p)| p.len());
        for a in &anchors {
            assert!(place_rank(kind, &a.segs) < known, "{} is a place ANCHOR_ORDER does not list", a.at);
        }
        anchors.into_iter().map(|a| a.at).collect::<Vec<_>>()
    };
    let flow = "schema: delurio-graph/0
kind: flow
id: x
source: a.ts
initial: s
states:
  s:
    source: a.ts:1
    on:
      GO: { target: t, source: a.ts:2 }
      BOTH: [{ target: t, source: a.ts:3 }]
    always: [{ target: t, source: a.ts:4 }]
  t: { type: final }
assertions:
  - { id: a, always: \"true\", source: a.ts:5 }
";
    assert_eq!(
        at(SchemaKind::Flow, flow),
        ["(top)", "states.s", "states.s.on.GO", "states.s.on.BOTH.0", "states.s.always.0", "assertions.0"]
    );
    let ui = "schema: delurio-graph/0
kind: ui
id: x
flow: x
source: a.tsx
samples: { source: a.tsx:1 }
nodes:
  n: { type: text, in: [s], source: a.tsx:2 }
";
    assert_eq!(at(SchemaKind::Ui, ui), ["(top)", "nodes.n"]);
    let nav = "schema: delurio-graph/0
kind: nav
id: x
source: { app: src }
frames: { f: { source: a.tsx:1 } }
pages: { p: { route: /, source: a.tsx:2 } }
entries: [{ from: p, to: p, how: link, source: a.tsx:3 }]
";
    assert_eq!(at(SchemaKind::Nav, nav), ["pages.p", "frames.f", "entries.0"]);
    let domain = "schema: delurio-graph/0
kind: domain
id: x
source: a.ts
entities:
  E: { source: a.ts:1, fields: { f: { type: string, source: a.ts:2 } } }
actions:
  a:
    source: a.ts:3
    route: POST /a
    body: { type: object, source: a.ts:4 }
    query: { type: object, source: a.ts:5 }
    result: { returns: E, source: a.ts:6 }
    refusals:
      R: { status: 404, source: a.ts:7, body: { type: object, source: a.ts:8 } }
policies:
  - { actor: person, do: [a], source: a.ts:9 }
invariants:
  - { id: i, rule: r, source: a.ts:10 }
";
    assert_eq!(
        at(SchemaKind::Domain, domain),
        [
            "(top)",
            "entities.E",
            "entities.E.fields.f",
            "actions.a",
            "actions.a.body",
            "actions.a.query",
            "actions.a.result",
            "actions.a.refusals.R",
            "actions.a.refusals.R.body",
            "invariants.0",
            "policies.0",
        ]
    );
    // a value that is not `<path>[:<line>]`, or names a file outside the repository, is no anchor
    let words = "schema: delurio-graph/0\nkind: ui\nid: x\nflow: x\nsource: \"a b.tsx\"\nnodes:\n  n: { type: text, in: [s], source: ../a.tsx:2 }\n  m: { type: text, in: [s], source: \"a.tsx:x\" }\n  o: { type: text, in: [s], source: ./a.tsx:007 }\n";
    let doc = read_yaml(words).expect("the file reads");
    let found: Vec<(String, String, Option<u64>)> =
        anchors_of(SchemaKind::Ui, "f.yaml", &doc).into_iter().map(|a| (a.at, a.path, a.line)).collect();
    assert_eq!(found, [("nodes.o".to_string(), "a.tsx".to_string(), Some(7))]);
}

#[test]
fn git_u0_hunks_are_read_for_each_side_a_deletion_and_a_rename_included() {
    let patch = [
        "diff --git a/src/a.ts b/src/a.ts",
        "--- a/src/a.ts",
        "+++ b/src/a.ts",
        "@@ -3 +3,2 @@ x",
        "-- a removed line that reads as a header",
        "--- a/elsewhere.ts",
        "+++ b/elsewhere.ts",
        "@@ -10,2 +11,0 @@",
        "diff --git a/src/gone.ts b/src/gone.ts",
        "--- a/src/gone.ts",
        "+++ /dev/null",
        "@@ -1,4 +0,0 @@",
        "diff --git a/old.ts b/new.ts",
        "similarity index 90%",
        "rename from old.ts",
        "rename to new.ts",
        "--- a/old.ts",
        "+++ b/new.ts",
        "@@ -2 +2 @@",
        "diff --git \"a/sp ace\\303\\266.ts\" \"b/sp ace\\303\\266.ts\"",
        "--- \"a/sp ace\\303\\266.ts\"",
        "+++ \"b/sp ace\\303\\266.ts\"",
        "@@ -1 +1 @@",
        "diff --git a/img.png b/img.png",
        "Binary files a/img.png and b/img.png differ",
    ]
    .join("\n");
    let h = |old_start, old_lines, new_start, new_lines| Hunk { old_start, old_lines, new_start, new_lines };
    let got: Vec<(String, Vec<Hunk>)> = parse_hunks(&patch).into_iter().collect();
    assert_eq!(
        got,
        [
            ("src/a.ts".to_string(), vec![h(3, 1, 3, 2), h(10, 2, 11, 0)]),
            ("src/gone.ts".to_string(), vec![h(1, 4, 0, 0)]),
            ("new.ts".to_string(), vec![h(2, 1, 2, 1)]),
            ("sp aceö.ts".to_string(), vec![h(1, 1, 1, 1)]),
        ]
    );
}

#[test]
fn a_hunk_touches_the_lines_it_changes_and_an_insertion_or_deletion_the_lines_beside_it() {
    let h = |old_start, old_lines, new_start, new_lines| Hunk { old_start, old_lines, new_start, new_lines };
    // lines 3 and 4 replaced by three lines
    assert!(!hunk_touches(&h(3, 2, 3, 3), 2));
    assert!(hunk_touches(&h(3, 2, 3, 3), 3));
    assert!(hunk_touches(&h(3, 2, 3, 3), 5));
    assert!(!hunk_touches(&h(3, 2, 3, 3), 6));
    // a line inserted after line 5: lines 5 and 6 are beside it
    assert!(!hunk_touches(&h(5, 0, 6, 1), 4));
    assert!(hunk_touches(&h(5, 0, 6, 1), 5));
    assert!(hunk_touches(&h(5, 0, 6, 1), 6));
    assert!(!hunk_touches(&h(5, 0, 6, 1), 7));
}

#[test]
fn a_word_stands_apart_from_letters_digits_and_underscores() {
    assert!(word_in(r#"{"returns":"Order"}"#, "Order"));
    assert!(!word_in(r#"{"returns":"Orders"}"#, "Order"));
    assert!(!word_in("_Order", "Order"));
    assert!(word_in("refund gives back", "refund"));
    assert!(!word_in("refunds-are-whole", "refund"));
    assert!(word_in("Ödeme Order.", "Order"));
}

#[test]
fn the_graphs_folder_is_read_without_a_leading_dot_or_slashes() {
    assert_eq!(trim_graphs("./graphs/"), "graphs");
    assert_eq!(trim_graphs("/graphs"), "graphs");
    assert_eq!(trim_graphs(".graphs"), ".graphs");
    assert_eq!(trim_graphs(""), "");
    assert!(inside("", "a/b"));
    assert!(inside("graphs", "graphs/a"));
    assert!(!inside("graphs", "graphs2/a"));
}
