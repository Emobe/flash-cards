//! Shared by the test files that run the real `fc` binary.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// A collection file in the temp directory, removed (with its installation file) when dropped.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn new(tag: &str, name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-{tag}-{}-{name}", std::process::id())))
    }

    /// A new, empty collection.
    pub fn collection(tag: &str, name: &str) -> Self {
        let file = Self::new(tag, name);
        out(&fc(&["new", file.path()]));
        file
    }

    pub fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}.installation", self.path()));
    }
}

pub fn fc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fc"))
        .args(args)
        .output()
        .unwrap()
}

/// What a command printed, which must have worked.
pub fn out(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What a command complained about, which must have failed.
pub fn err(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "expected a failure, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The ID in parentheses at the end of the first line of `text`, as in "Added deck X (ID)".
pub fn id_in(text: &str) -> String {
    let line = text.lines().next().unwrap();
    let start = line.rfind('(').unwrap() + 1;
    line[start..line.rfind(')').unwrap()].to_owned()
}

/// The ID after "Added note " in `fc add-note`'s answer.
pub fn note_id(text: &str) -> String {
    text.strip_prefix("Added note ")
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .to_owned()
}
