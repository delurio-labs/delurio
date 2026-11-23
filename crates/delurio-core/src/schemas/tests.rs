use super::*;

#[test]
fn every_schema_compiles() {
    for kind in KINDS {
        let v = validator(kind);
        assert_eq!(v.pointer(v.root()), "");
    }
}

// each file of a folder made for the test, with what the schema rule says of it: `<line>:<column> <message>`
fn findings_of(name: &str, files: &[(&str, &str)]) -> Vec<(String, Vec<String>)> {
    let dir = std::env::temp_dir().join(format!("delurio-schemas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder is made");
    let mut out = Vec::new();
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::write(&path, text).expect("the file is written");
        let path = path.to_string_lossy().into_owned();
        let said = validate_file(&path).iter().map(|f| format!("{}:{} {}", f.line, f.column, f.message)).collect();
        out.push((file.to_string(), said));
    }
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn a_binding_is_known_by_its_name() {
    assert_eq!(schema_kind_of("graphs/team/binding.yaml"), Some(SchemaKind::Binding));
    assert_eq!(schema_kind_of("graphs/replay.base.binding.yaml"), Some(SchemaKind::Binding));
    assert_eq!(schema_kind_of("graphs/team/binding.noSessions.yaml"), Some(SchemaKind::Binding));
    assert_eq!(schema_kind_of("graphs/team/binding.common.yaml"), Some(SchemaKind::Binding));
    assert!(!is_binding_name("binding..yaml"));
    assert!(!is_binding_name("bindings.yaml"));
    assert!(!is_binding_name("binding.yml"));
    assert_eq!(schema_kind_of("graphs/team/tutorial.yaml"), None);
}

#[test]
fn a_binding_of_a_flow_of_a_navigation_graph_and_a_shared_one_follow_the_schema() {
    let flow = r##"schema: delurio-graph/0
kind: binding
flow: demo.team
extends: ../replay.base.binding.yaml
apps: { app: "http://127.0.0.1:3000" }
browser: { width: 390, height: 844, locale: tr-TR, timeout: 20, ignoreHTTPSErrors: true }
data: { seed: { json: .work/seed.json }, env: { dotenv: .work/replay.env } }
accounts:
  owner: { address: owner@example.test, pin: 1234 }
signIn:
  - goto: "{app}/login"
  - fill: { label: Email, value: "{address}" }
  - click: { role: button, name: /Sign in|Giriş/ }
scenarios:
  owner:
    given: { role: owner, taken: false }
    as: owner
    open: "{app}/team"
    vars: { invitee: "ada@example.test" }
    setup: [{ run: [node, seed.mjs, 3] }]
    viewport: { width: 390, height: 844 }
    then: { next: [{ lastStep: false }, { lastStep: true, found: null }] }
  entered:
    given: { role: owner, taken: false }
    open: "{app}/"
    enter: { nav: ../nav, entry: "frame:workspace->team" }
events:
  NAVIGATE: [{ goto: "{app}/elsewhere", as: owner }, { reload: true }, { key: Escape }]
states:
  teamPage: { url: "{app}/team", text: Members }
  inviting:
    call: { url: "{app}/api/invite", method: POST, code: error.code, returns: { DONE: { path: ok, equals: true } } }
    fail: { FAIL.network: abort, FAIL.unavailable: { status: 503, body: '{"detail":"{message}"}' }, FAIL.timeout: hang }
  left: { url: "{app}/elsewhere", before: [{ waitResponse: { url: "{app}/api/*", status: 200 } }] }
  filled: { storage: { session: { "rehber:*": absent }, local: { theme: /^(dark|light)$/, visits: 3 } } }
  sending: { call: { url: "{shop}/orders", method: POST, status: [201, 409] } }
  inert: { call: { url: "{embed}/rehber/v1/*/guides*", status: 404 } }
nodes:
  memberItem: { count: "{seed.members}" }
  removeButton: { role: button, name: Remove, within: { css: "#members" }, nth: 0, before: [{ click: { testId: menu } }] }
  moveUp:
    role: button
    name: Move up
    before: { listPage: [{ click: { role: button, name: Actions } }], rowPage: [] }
  saveButton: { before: [{ click: { role: button, name: Dismiss }, optional: true }, { fill: { label: Note, value: x }, optional: true }] }
  avatar: { skip: drawn on a canvas }
  newestRow: { role: row, within: { role: table, name: Weeks }, nth: -1 }
  stepBox: { near: { role: button, name: Order } }
  screenPart:
    role: dialog
    name: Order a coffee
    draws: { flow: ../cafe/order, bind: ../cafe/order/binding.yaml, given: { oat: true }, states: { sentPage: done } }
effects:
  sendInvitation:
    - { run: [node, mail.mjs], expect: sent }
    - { attach: { label: Photo, file: [fixtures/a.png] } }
    - { attach: { css: "input[type=file]", run: [node, csv.mjs], fileName: grades.csv, mimeType: text/csv } }
    - download: { name: grades-*.csv, rows: 3, columns: [Name, Grade], cells: [{ row: 1, column: Name, equals: Ada }, { cell: B2, equals: 90 }] }
    - { resize: { width: 800, height: 600 } }
settle: { quiet: 300, max: 8000 }
parallel: { walks: auto, serial: [[owner, entered]] }
session: { check: { open: "{app}/account", find: { role: heading, name: Account } }, maxAge: 12h, fresh: [entered] }
snapshot: { save: [{ run: [node, save.mjs] }], restore: [{ run: [node, restore.mjs] }] }
mask: [credential, personal]
pageErrors: report
"##;
    let nav = r#"kind: binding
nav: demo.panel
apps: { panel: "http://panel.test" }
accounts: { owner: { address: owner@example.test } }
setup: [{ run: [psql, -c, "select 1"] }]
personas:
  viewer: { as: owner, setup: [{ run: [psql, -c, "update membership set role = 'viewer'"] }] }
frames:
  workspace:
    find: { role: navigation, name: Main menu }
    user: { open: [{ click: { css: "button.avatar" } }], find: { role: menu } }
pages:
  signIn: { skip: the realm's page }
  connections: { refusal: { text: Only an owner manages its connections. }, samples: { workspace: ws-1 } }
entries:
  "frame:workspace->signOut": { follow: false }
  "overview->projects": { role: link, name: All projects }
parallel: 2
"#;
    let shared = r#"kind: binding
mask: []
signIn: [{ goto: "{app}/login" }]
browser: { locale: tr-TR }
session: { check: { open: "{app}/", find: { role: main } } }
"#;
    let found = findings_of(
        "good",
        &[("binding.yaml", flow), ("panel.nav.binding.yaml", nav), ("replay.base.binding.yaml", shared)],
    );
    for (file, said) in found {
        assert_eq!(said, Vec::<String>::new(), "{file}");
    }
}

#[test]
fn a_binding_that_does_not_follow_the_schema_is_told_where_and_why() {
    let text = r#"kind: binding
flow: demo.team
scenarious:
  owner: { open: "{app}/team" }
scenarios:
  owner:
    open: "{app}/team"
    setup: [{ click: { role: button, name: Reset } }]
  member: { given: { role: member } }
  next: { open: "{app}/", then: { next: { lastStep: true } } }
events:
  NAVIGATE:
    - { goto: "{app}/x", click: { role: link, name: Away } }
    - { key: Enter, expect: done }
    - { as: owner }
states:
  inviting: { fail: { FAIL.network: abort } }
  sent: { call: { url: "{app}/api", methd: POST } }
  kept: { storage: { cookie: { a: b } } }
  posted: { call: { url: "{app}/x", status: 99 } }
  shown: { storage: { session: {} } }
nodes:
  approve: { role: button, name: Approve, exact: true }
  part: { draws: { flow: ../cafe/order, state: done } }
  box: { near: { nth: 1 } }
  moved: { before: { listPage: { click: { role: button } } } }
  kept: { before: {} }
  left: { before: [{ goto: "{app}/", optional: true }] }
effects:
  sendInvitation: [{ attach: { label: File, run: [node, gen.mjs] } }]
pageErrors: warn
mask: [secret, credential, credential]
"#;
    // in the order the schema finds them; check sorts every finding by its place
    let found = findings_of("bad", &[("binding.yaml", text)]);
    assert_eq!(
        found[0].1,
        [
            r#"3:1 the file: "scenarious" is not a key here; did you mean "scenarios"?"#,
            r#"32:8 mask.0: must be "credential", "personal" or "internal""#,
            r#"32:7 mask: must NOT have duplicate items (items ## 1 and 2 are identical)"#,
            r#"31:13 pageErrors: must be "fail" or "report""#,
            r#"8:13 scenarios.owner.setup.0: "run" is missing (only commands run here: there is no page)"#,
            r#"9:11 scenarios.member: "open" is missing"#,
            r#"10:41 scenarios.next.then.next: must be a list"#,
            r#"13:33 events.NAVIGATE.0.click: is not allowed here (an action does one thing: give each its own item)"#,
            r#"14:7 events.NAVIGATE.1: "run" is missing (expect goes with run)"#,
            concat!(
                r#"15:7 events.NAVIGATE.2: "goto" or "reload" or "click" or "fill" or "attach" or "key" or "waitUrl" or "#,
                r#""waitText" or "waitResponse" or "download" or "resize" or "run" is missing"#
            ),
            r#"17:13 states.inviting: "call" is missing (fail makes the state's call fail, so it needs call)"#,
            r#"18:37 states.sent.call: "methd" is not a key here; did you mean "method"?"#,
            r#"19:22 states.kept.storage: "cookie" is not a key here"#,
            r#"20:45 states.posted.call.status: must be >= 100"#,
            r#"21:32 states.shown.storage.session: needs at least 1 key"#,
            r#"23:43 nodes.approve: "exact" is not a key here"#,
            concat!(
                r#"24:9 nodes.part: "role" or "text" or "label" or "testId" or "css" is missing (draws finds the element "#,
                r#"that holds the drawn screen by the node's own locator)"#
            ),
            r#"24:41 nodes.part.draws: "state" is not a key here; did you mean "states"?"#,
            r#"25:16 nodes.box.near: "role" or "text" or "label" or "testId" or "css" is missing"#,
            r#"26:32 nodes.moved.before.listPage: must be a list"#,
            r#"27:19 nodes.kept.before: needs at least 1 key"#,
            r#"28:20 nodes.left.before.0: "click" or "fill" or "attach" is missing (optional goes with click, fill or attach)"#,
            r#"30:30 effects.sendInvitation.0.attach: "fileName" is missing (run gives a file of the name fileName)"#,
        ]
    );
}
