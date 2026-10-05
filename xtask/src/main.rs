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
  bindings
          Regenerate the TypeScript bindings in packages/core-client/src/generated from Rust
  wasm [--release]
          Build fc-wasm for the browser and generate its JS glue into apps/web/src/wasm
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
        Some("bindings") => bindings(),
        Some("wasm") => wasm(&env::args().skip(2).collect::<Vec<_>>()),
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
    run(
        "cargo",
        &[
            "clippy",
            "-p",
            "fc-wasm",
            "--target",
            "wasm32-unknown-unknown",
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
    // The generated .d.ts must exist before `tsc` checks apps/web.
    wasm(&[])?;
    run("bun", &["run", "typecheck"])?;
    run("bun", &["run", "test"])?;
    println!("\nAll checks passed.");
    Ok(())
}

fn fmt() -> Result<(), String> {
    run("cargo", &["fmt", "--all"])?;
    run("bun", &["run", "format"])
}

/// Rewrites the generated TypeScript bindings. The `fc-api` test does the generating (it is also
/// what fails `check` when the committed files are stale), then Biome formats the output.
fn bindings() -> Result<(), String> {
    println!("\n==> cargo test -p fc-api (FC_UPDATE_BINDINGS=1)");
    let status = Command::new("cargo")
        .args([
            "test",
            "-p",
            "fc-api",
            "--locked",
            "committed_bindings_are_current",
        ])
        .env("FC_UPDATE_BINDINGS", "1")
        .current_dir(repo_root())
        .status()
        .map_err(|e| format!("could not start `cargo`: {e}"))?;
    if !status.success() {
        return Err(format!("generating bindings failed ({status})"));
    }
    run(
        "bun",
        &[
            "x",
            "biome",
            "check",
            "--write",
            "packages/core-client/src/generated",
        ],
    )
}

/// Builds `fc-wasm` and generates the JS glue. Stops early when the `wasm-bindgen` CLI does not match
/// the crate version in `Cargo.lock`, since a mismatch fails with an unreadable error.
fn wasm(args: &[String]) -> Result<(), String> {
    let release = match args {
        [] => false,
        [flag] if flag == "--release" => true,
        _ => return Err("usage: cargo xtask wasm [--release]".to_owned()),
    };
    let wanted = locked_version("wasm-bindgen")?;
    let install = format!("cargo install wasm-bindgen-cli --version {wanted} --locked");
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|_| format!("`wasm-bindgen` is not installed or not on PATH. Run: {install}"))?;
    let found = String::from_utf8_lossy(&output.stdout);
    if found.split_whitespace().nth(1) != Some(wanted.as_str()) {
        return Err(format!(
            "`wasm-bindgen` is \"{}\", but Cargo.lock needs {wanted}. Run: {install}",
            found.trim()
        ));
    }
    let mut build = vec![
        "build",
        "-p",
        "fc-wasm",
        "--target",
        "wasm32-unknown-unknown",
        "--locked",
    ];
    if release {
        build.push("--release");
    }
    run("cargo", &build)?;
    let profile = if release { "release" } else { "debug" };
    let module = format!("target/wasm32-unknown-unknown/{profile}/fc_wasm.wasm");
    run(
        "wasm-bindgen",
        &["--target", "web", "--out-dir", "apps/web/src/wasm", &module],
    )
}

/// The version of package `name` recorded in `Cargo.lock`.
fn locked_version(name: &str) -> Result<String, String> {
    let lock = fs::read_to_string(repo_root().join("Cargo.lock"))
        .map_err(|e| format!("reading Cargo.lock: {e}"))?;
    let header = format!("name = \"{name}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == header
            && let Some(version) = lines.next().and_then(|l| l.strip_prefix("version = \""))
        {
            return Ok(version.trim_end_matches('"').to_owned());
        }
    }
    Err(format!("{name} is not in Cargo.lock"))
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
