//! Repository tasks, run with `cargo xtask <task>`.
//!
//! Uses only the standard library so it builds fast and runs the same on
//! Linux and Windows.

mod doctor_android;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  check   Run every check: lockfile guard, Rust fmt/clippy/test/deny, Bun install/lint/typecheck/test
  fmt     Format all Rust and JS/TS code in place
  doctor-android
          Check the Android toolchain setup (SDK, NDK, JDK, Rust target, device)";

/// Lockfiles from package managers other than Bun. See "Tooling constraints"
/// in docs/PRODUCT.md.
const FORBIDDEN_LOCKFILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
];

/// Directories never searched for lockfiles.
const SKIPPED_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", "gen"];

fn main() -> ExitCode {
    let task = env::args().nth(1);
    let result = match task.as_deref() {
        Some("check") => check(),
        Some("fmt") => fmt(),
        Some("doctor-android") => doctor_android::run(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nxtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn check() -> Result<(), String> {
    step("lockfile guard", check_lockfiles)?;
    run("cargo", &["fmt", "--all", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--workspace", "--locked"])?;
    run("cargo", &["deny", "--locked", "check"])?;
    run("bun", &["install", "--frozen-lockfile"])?;
    run("bun", &["run", "lint"])?;
    run("bun", &["run", "typecheck"])?;
    run("bun", &["run", "test"])?;
    println!("\nAll checks passed.");
    Ok(())
}

fn fmt() -> Result<(), String> {
    run("cargo", &["fmt", "--all"])?;
    run("bun", &["run", "format"])
}

fn step(name: &str, f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    println!("\n==> {name}");
    f()
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    println!("\n==> {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(repo_root())
        .status()
        .map_err(|e| format!("could not start `{program}`: {e}. Is it installed and on PATH?"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program} {}` failed ({status})", args.join(" ")))
    }
}

fn check_lockfiles() -> Result<(), String> {
    let mut found = Vec::new();
    find_forbidden_lockfiles(&repo_root(), &mut found)?;
    if found.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = found.iter().map(|p| format!("  {}", p.display())).collect();
    Err(format!(
        "found lockfiles from a package manager other than Bun:\n{}\nDelete them and use `bun install` (see docs/PRODUCT.md, Tooling constraints).",
        list.join("\n")
    ))
}

fn find_forbidden_lockfiles(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_ref()) {
                find_forbidden_lockfiles(&path, found)?;
            }
        } else if FORBIDDEN_LOCKFILES.contains(&name.as_ref()) {
            found.push(path);
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repository root")
        .to_path_buf()
}
