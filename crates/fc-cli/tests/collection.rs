//! Runs the real `fc` binary: create, open and close a collection, and the failure messages.

use std::path::PathBuf;
use std::process::{Command, Output};

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("fc-cli-{}-{name}.db", std::process::id())))
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn installation(&self) -> PathBuf {
        PathBuf::from(format!("{}.installation", self.path()))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.installation());
    }
}

fn fc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fc"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn creates_then_opens_and_closes_a_collection() {
    let file = TempFile::new("roundtrip");
    let created = fc(&["new", file.path()]);
    assert!(created.status.success(), "{}", stderr(&created));
    assert!(file.0.exists());

    let info = fc(&["info", file.path()]);
    assert!(info.status.success(), "{}", stderr(&info));
    let text = stdout(&info);
    assert!(text.contains("Storage version: 2 (this build understands up to 2)"));
    assert!(text.contains("Device ID: "));
    assert!(text.contains("Created by core: 0.0.0"));
}

#[test]
fn refuses_to_overwrite_and_to_open_what_is_missing() {
    let file = TempFile::new("refuse");
    assert!(fc(&["new", file.path()]).status.success());

    let again = fc(&["new", file.path()]);
    assert_eq!(again.status.code(), Some(1));
    assert!(stderr(&again).contains("already exists"));

    let missing = TempFile::new("missing");
    let info = fc(&["info", missing.path()]);
    assert_eq!(info.status.code(), Some(1));
    assert!(stderr(&info).contains("No collection exists"));
    assert!(!missing.0.exists());
    assert!(!missing.installation().exists());
}

fn device_id(output: &Output) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("Device ID: "))
        .unwrap()
        .to_owned()
}

#[test]
fn the_device_id_is_kept_and_a_copied_collection_gets_its_own() {
    let original = TempFile::new("device-original");
    assert!(fc(&["new", original.path()]).status.success());
    let id = device_id(&fc(&["info", original.path()]));
    assert_eq!(id.len(), 36);
    assert_eq!(device_id(&fc(&["info", original.path()])), id);

    // Only the .db is copied, as when a collection is sent to another machine.
    let copy = TempFile::new("device-copy");
    std::fs::copy(&original.0, &copy.0).unwrap();
    assert_ne!(device_id(&fc(&["info", copy.path()])), id);
    // The original is not affected by the copy.
    assert_eq!(device_id(&fc(&["info", original.path()])), id);

    // Copying the installation file too is not a different installation: same device ID. This
    // is why a restore must regenerate it explicitly (step 1.13).
    let twin = TempFile::new("device-twin");
    std::fs::copy(&original.0, &twin.0).unwrap();
    std::fs::copy(original.installation(), twin.installation()).unwrap();
    assert_eq!(device_id(&fc(&["info", twin.path()])), id);
}

#[test]
fn a_collection_from_a_newer_version_fails_with_an_update_message() {
    let file = TempFile::new("newer");
    assert!(fc(&["new", file.path()]).status.success());
    // There is no CLI command to fake this, so set the version the way a newer app would have.
    let bytes = std::fs::read(&file.0).unwrap();
    let mut patched = bytes.clone();
    // `user_version` is the big-endian u32 at byte offset 60 of the SQLite header.
    patched[60..64].copy_from_slice(&99u32.to_be_bytes());
    std::fs::write(&file.0, &patched).unwrap();

    let info = fc(&["info", file.path()]);
    assert_eq!(info.status.code(), Some(1));
    assert!(stderr(&info).contains("newer version of the app"));
    assert_eq!(std::fs::read(&file.0).unwrap(), patched);
}

#[test]
fn bad_usage_exits_with_2_and_prints_the_usage() {
    for args in [&[][..], &["nope"], &["new"], &["info", "a", "b"]] {
        let output = fc(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).contains("Usage:"), "{args:?}");
    }
    assert!(fc(&["help"]).status.success());
}
