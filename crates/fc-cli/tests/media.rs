//! Runs the real `fc` binary: media.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-media-{}-{name}", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn with(name: &str, bytes: &[u8]) -> Self {
        let file = Self::new(name);
        std::fs::write(&file.0, bytes).unwrap();
        file
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

/// The name `add-media` printed: the word after "Added " or "Use ".
fn name_in(text: &str) -> String {
    let after = text
        .strip_prefix("Added ")
        .or_else(|| text.split("Use ").nth(1))
        .unwrap();
    after.split([' ', '(']).next().unwrap().to_owned()
}

#[test]
fn adds_a_file_once_and_gives_the_same_bytes_back() {
    let db = TempFile::new("add.db");
    out(&fc(&["new", db.path()]));
    let picture = TempFile::with("cat.png", b"cat picture bytes");
    let same = TempFile::with("copy of cat.png", b"cat picture bytes");

    let first = out(&fc(&["add-media", db.path(), picture.path()]));
    assert!(first.starts_with("Added "), "{first}");
    let name = name_in(&first);
    assert!(name.ends_with(".png"), "{name}");
    assert!(first.contains(&format!("<img src=\"{name}\">")), "{first}");

    let second = out(&fc(&["add-media", db.path(), same.path()]));
    assert!(second.contains("already has these bytes"), "{second}");

    let list = out(&fc(&["media", db.path()]));
    assert!(list.starts_with("1 media file in the collection"), "{list}");
    assert!(
        list.contains("17 bytes") && list.contains("unused"),
        "{list}"
    );

    let copy = TempFile::new("copy.out");
    let wrote = out(&fc(&["media-get", db.path(), &name, copy.path()]));
    assert!(wrote.contains("17 bytes"), "{wrote}");
    assert_eq!(std::fs::read(copy.path()).unwrap(), b"cat picture bytes");
}

#[test]
fn finds_unused_and_missing_media_and_deletes_the_unused() {
    let db = TempFile::new("check.db");
    out(&fc(&["new", db.path()]));
    let used = TempFile::with("used.png", b"used bytes");
    let spare = TempFile::with("spare.mp3", b"spare bytes");
    let used_name = name_in(&out(&fc(&["add-media", db.path(), used.path()])));
    out(&fc(&["add-media", db.path(), spare.path()]));
    out(&fc(&[
        "add-note",
        db.path(),
        "Basic",
        &format!("Front=<img src=\"{used_name}\"> <img src=\"lost.jpg\">"),
        "Back=x",
    ]));

    let list = out(&fc(&["media", db.path()]));
    assert!(list.starts_with("2 media files"), "{list}");
    assert!(
        list.contains(&format!("used  {used_name}")) && list.contains("unused"),
        "{list}"
    );

    let check = out(&fc(&["media-check", db.path()]));
    assert!(
        check.starts_with("1 unused, 1 missing, 0 whose bytes"),
        "{check}"
    );
    assert!(
        check.contains("unused: ") && check.contains("11 bytes"),
        "{check}"
    );
    assert!(check.contains("missing: lost.jpg  notes: "), "{check}");

    let deleted = out(&fc(&["delete-unused-media", db.path()]));
    assert_eq!(deleted.trim(), "Deleted 1 unused media file");
    let check = out(&fc(&["media-check", db.path()]));
    assert!(check.starts_with("0 unused, 1 missing"), "{check}");
    let list = out(&fc(&["media", db.path()]));
    assert!(list.contains("deleted"), "{list}");
    let deleted = out(&fc(&["delete-unused-media", db.path()]));
    assert_eq!(deleted.trim(), "Deleted 0 unused media files");
}

#[test]
fn says_what_is_wrong_with_a_file_or_a_name() {
    let db = TempFile::new("errors.db");
    out(&fc(&["new", db.path()]));
    let empty = TempFile::with("empty.png", b"");
    let message = err(&fc(&["add-media", db.path(), empty.path()]));
    assert!(message.contains("is empty"), "{message}");
    let message = err(&fc(&["add-media", db.path(), "/no/such/file.png"]));
    assert!(message.contains("Cannot read"), "{message}");
    let target = TempFile::new("never.out");
    let message = err(&fc(&["media-get", db.path(), "nothing.png", target.path()]));
    assert!(
        message.contains("No media file is called \"nothing.png\""),
        "{message}"
    );
}
