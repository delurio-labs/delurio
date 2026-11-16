// What the CI of the TypeScript checker asserts on examples/ (.github/workflows/ci.yml), asserted on the Rust port: the
// focus after a step, the paths that cover a flow, and a tutorial that no longer matches its flow.

// a file read here returns the crate's error as it is, not boxed, as the crate does
#![allow(clippy::result_large_err)]

use std::collections::HashSet;

use delurio_core::graph::{FocusBy, file_exists, focus_after, load, load_with, read_file};
use delurio_core::model::{By, Config, Model, Step, cover, explore};
use delurio_core::tutorial_check::check_tutorial;

fn example(dir: &str) -> String {
    format!("{}/../../examples/{dir}", env!("CARGO_MANIFEST_DIR"))
}

// "The focus after a step is the first of returnsFocus, initialFocus and focus"
#[test]
fn the_focus_after_a_step() {
    let edit = load(&example("notes/edit")).unwrap();
    let at = |from: Option<&str>, to: &str| {
        focus_after(edit.ui(), from, to, None).map(|f| {
            let by = match f.by {
                FocusBy::ReturnsFocus => "returnsFocus",
                FocusBy::InitialFocus => "initialFocus",
                FocusBy::Focus => "focus",
            };
            format!("{by} {}", f.node)
        })
    };
    assert_eq!(at(None, "editing").as_deref(), Some("initialFocus title"));
    assert_eq!(at(Some("editing"), "discarding").as_deref(), Some("initialFocus discardTitle"));
    assert_eq!(at(Some("discarding"), "editing").as_deref(), Some("returnsFocus cancel"));
    assert_eq!(at(Some("saving"), "saved").as_deref(), Some("focus savedStatus"));
    assert_eq!(at(Some("editing"), "saving"), None);
    assert_eq!(at(Some("editing"), "editing"), None);
}

// "Paths cover every reachable state and every transition, the same in every run"
#[test]
fn paths_cover_every_reachable_state_and_transition() {
    for dir in ["qauth-authorize", "notes/edit", "notes/restore", "notes/archive", "sign-out"] {
        let m = Model::new(load(&example(dir)).unwrap().flow);
        let ex = explore(&m, None, None).unwrap();
        let states: HashSet<String> = ex.configs.iter().map(|c| c.state.to_string()).collect();
        let transitions: HashSet<String> = ex.taken.iter().map(|t| t.to_string()).collect();
        for (by, want) in [(By::States, states), (By::Transitions, transitions)] {
            let c = cover(&m, by, None, None).unwrap();
            assert_eq!(cover(&m, by, None, None).unwrap(), c, "{dir} {by:?}: the same paths in every run");
            let passed: HashSet<&String> =
                c.paths.iter().flat_map(|p| if by == By::States { &p.states } else { &p.transitions }).collect();
            for x in &want {
                assert!(passed.contains(x), "{dir} {by:?}: {x} is on no path");
            }
            for p in &c.paths {
                assert!(p.id.len() == 14 && p.id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            }
            assert!(c.unreached.is_empty(), "{dir} {by:?}: everything is reached");
        }
    }
    let m = Model::new(load(&example("notes/edit")).unwrap().flow);
    let keep = |_: &Config, s: &Step| s.event.as_deref() != Some("CANCEL");
    let driven = cover(&m, By::States, Some(m.initials()), Some(&keep)).unwrap();
    assert_eq!(driven.unreached, ["discarding", "left"]);
}

/// the problems of notes/edit's tutorial once one line of a file is changed, as the CI's `sed` changes it: the TS
/// checkTutorial's problems on the same change (node, scratch tut-ci.mjs)
fn tutorial_after(file: &str, line: &str, to: &str) -> Vec<(Vec<String>, String)> {
    let dir = example("notes/edit");
    let edited = |f: &str| {
        read_file(&dir, f).map(|t| {
            if f != file {
                return t;
            }
            t.split('\n')
                .map(|l| if l.contains(line) { l.replacen(line, to, 1) } else { l.to_string() })
                .collect::<Vec<_>>()
                .join("\n")
        })
    };
    let g = load_with(&dir, &edited, &|f| file_exists(&dir, f)).unwrap();
    let m = Model::new(g.flow.clone());
    let ex = explore(&m, None, None).unwrap();
    let text = edited("tutorial.yaml").unwrap();
    check_tutorial(&g, &m, &ex, &text).unwrap().into_iter().map(|p| (p.path, p.message)).collect()
}

fn one(path: &[&str], message: &str) -> Vec<(Vec<String>, String)> {
    vec![(path.iter().map(|s| s.to_string()).collect(), message.to_string())]
}

// "A tutorial that no longer matches its flow fails the check at its step"
#[test]
fn a_tutorial_that_no_longer_matches_its_flow_stops_at_its_step() {
    let t = "tutorial.yaml";
    assert_eq!(tutorial_after(t, "", ""), Vec::new());
    assert_eq!(
        tutorial_after(t, "    press: save", "    press: submit"),
        one(&["steps", "3", "press"], "step 4: press submit, which is not a node of ui.yaml")
    );
    assert_eq!(
        tutorial_after(t, "after: [DONE]", "after: [FINISHED]"),
        one(&["steps", "4", "after", "0"], "step 5: after FINISHED, which no state of example.editNote handles")
    );
    assert_eq!(
        tutorial_after(t, "{pinBox}", "{pinButton}"),
        one(&["steps", "2", "say"], "step 3: {pinButton} is neither a sample value nor a node of ui.yaml")
    );
    assert_eq!(
        tutorial_after(t, "scenario: { pinned: false }", "scenario: { pinned: maybe }"),
        one(&["scenario", "pinned"], "scenario: \"maybe\" is not a value of pinned (false, true)")
    );
    assert_eq!(
        tutorial_after(t, "  - show: saving", "  - show: editing"),
        one(&["steps", "4", "show"], "step 5: shows editing, and the flow is in saving here")
    );
    assert_eq!(
        tutorial_after("ui.yaml", "    disabledIn: [saving]", "    disabledIn: [editing, saving]"),
        one(&["steps", "3", "press"], "step 4: press save, which is disabled in editing")
    );
    assert_eq!(
        tutorial_after("flow.yaml", "      DONE: saved", "      DONE: editing"),
        one(&["steps", "5", "end"], "step 6: the tutorial ends in saved, and the flow is in editing here")
    );
}

#[test]
fn a_tutorial_on_the_diagram_walks_to_its_states() {
    let g = load(&example("sign-out")).unwrap();
    let m = Model::new(g.flow.clone());
    let ex = explore(&m, None, None).unwrap();
    let text = "schema: delurio-graph/0\nkind: tutorial\nid: example.signOut.tutorial\nflow: example.signOut\ntitle: Signing out\nstage: diagram\nscenario: { token: valid, hasCookie: true }\nsteps:\n  - show: checking\n    say: \"The app sends the token it holds.\"\n  - end: signed_out.cookie_cleared\n    say: \"The session ends, and the cookie with it.\"\n";
    assert_eq!(check_tutorial(&g, &m, &ex, text).unwrap(), Vec::new());
    let broken = text.replace("  - end: signed_out.cookie_cleared", "  - end: signed_out");
    let problems: Vec<_> =
        check_tutorial(&g, &m, &ex, &broken).unwrap().into_iter().map(|p| (p.path, p.message)).collect();
    assert_eq!(
        problems,
        one(
            &["steps", "1", "end"],
            "step 2: signed_out is not reached from checking without an event (the way stops in signed_out.cookie_cleared)"
        )
    );
}
