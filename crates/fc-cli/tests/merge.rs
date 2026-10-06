//! Runs the real `fc` binary: merging two collection files.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-merge-{}-{name}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}.installation", self.path()));
    }
}

fn fc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fc"))
        .args(args)
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_note_on_each_of_two_files_is_on_both_after_merging_both_ways() {
    let (a, b) = (TempFile::new("a"), TempFile::new("b"));
    out(&fc(&["new", a.path()]));
    out(&fc(&["new", b.path()]));
    out(&fc(&[
        "add-note",
        a.path(),
        "Basic",
        "Front=from A",
        "Back=1",
    ]));
    out(&fc(&[
        "add-note",
        b.path(),
        "Basic",
        "Front=from B",
        "Back=2",
    ]));

    let report = out(&fc(&["merge", a.path(), b.path()]));
    assert!(report.starts_with("Merged "), "{report}");
    assert!(report.contains("Notes reconciled:"), "{report}");
    assert!(report.contains("Registers applied: "), "{report}");
    let report = out(&fc(&["merge", b.path(), a.path()]));
    assert!(!report.contains("could not be applied"), "{report}");

    for file in [&a, &b] {
        let notes = out(&fc(&["notes", file.path()]));
        assert!(notes.contains("from A"), "{notes}");
        assert!(notes.contains("from B"), "{notes}");
    }
    // Merging again changes nothing.
    let again = out(&fc(&["merge", a.path(), b.path()]));
    assert!(again.contains("Registers applied: 0"), "{again}");
    assert!(again.contains("Rows added: 0"), "{again}");
}

#[test]
fn merging_a_file_into_itself_or_a_missing_file_is_refused() {
    let a = TempFile::new("self");
    out(&fc(&["new", a.path()]));
    assert!(err(&fc(&["merge", a.path(), a.path()])).contains("two different files"));
    let missing = TempFile::new("missing");
    assert!(!err(&fc(&["merge", a.path(), missing.path()])).is_empty());
    assert!(err(&fc(&["merge", a.path()])).contains("Wrong number of arguments"));
}
