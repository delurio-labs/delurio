//! The reference CI's example checks, held against this crate's binary: every `tests/differential/cases/ci-*.sh` (the
//! steps of the TypeScript checker's ci.yml, with their greps and exit-code tests: they encode SPEC.md) runs under
//! `bash -e`, in name order, in one shared temp folder, from a copy of the repository's examples, with `DELURIO` the
//! built binary and `TMPDIR_CASE` the shared folder. A script that fails fails the test. The scripts call `node` and
//! `python3` for their JSON and YAML edits, as the reference CI did.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_delurio");

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's folder is made");
    for entry in std::fs::read_dir(from).expect("the folder is read") {
        let entry = entry.expect("the folder's entry is read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry's type is read").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file is copied");
        }
    }
}

#[test]
fn the_reference_ci_steps_hold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases_dir = root.join("tests/differential/cases");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&cases_dir)
        .expect("the cases are there")
        .map(|e| e.expect("a case is read").path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("ci-") && n.ends_with(".sh")))
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no ci-*.sh under {}", cases_dir.display());

    // one shared folder for every case, as the reference CI had: the repository's examples, and the cases' own files
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join("behaviour");
    let _ = std::fs::remove_dir_all(&work);
    let repo = work.join("repo");
    let tmp = work.join("tmp");
    copy_dir(&root.join("examples"), &repo.join("examples"));
    std::fs::create_dir_all(&tmp).expect("the shared folder is made");

    let mut failed = Vec::new();
    for case in &cases {
        let name = case.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let out = Command::new("bash")
            .arg("-e")
            .arg(case)
            .current_dir(&repo)
            .env("DELURIO", BIN)
            .env("TMPDIR_CASE", &tmp)
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .output()
            .expect("bash runs");
        if !out.status.success() {
            let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let lines: Vec<&str> = said.lines().collect();
            let tail = &lines[lines.len().saturating_sub(30)..];
            failed.push(format!("{name}: {}\n{}", out.status, tail.join("\n")));
        }
    }
    eprintln!("{} cases ran, {} failed", cases.len(), failed.len());
    assert!(failed.is_empty(), "{} of the reference CI's steps failed:\n\n{}", failed.len(), failed.join("\n\n"));
}
