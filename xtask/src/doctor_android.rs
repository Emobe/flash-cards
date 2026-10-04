//! `cargo xtask doctor-android`: checks the Android toolchain setup described
//! in README.md and says exactly what is missing. Read-only; installs nothing.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::repo_root;

const RUST_TARGET: &str = "aarch64-linux-android";
const MIN_JAVA_MAJOR: u32 = 17;

struct Report {
    failures: usize,
}

impl Report {
    fn ok(&self, what: &str, detail: &str) {
        println!("  ok    {what}: {detail}");
    }

    fn fail(&mut self, what: &str, problem: &str, fix: &str) {
        self.failures += 1;
        println!("  FAIL  {what}: {problem}\n        fix: {fix}");
    }

    fn warn(&self, what: &str, problem: &str, fix: &str) {
        println!("  warn  {what}: {problem}\n        fix: {fix}");
    }
}

pub fn run() -> Result<(), String> {
    println!("Checking Android setup (see README.md, Android):\n");
    let mut report = Report { failures: 0 };

    let sdk = check_sdk(&mut report);
    check_ndk(&mut report);
    check_java(&mut report);
    check_rust_target(&mut report);
    if let Some(sdk) = &sdk {
        check_platform(&mut report, sdk);
        check_device(&report, sdk);
    }

    if report.failures == 0 {
        println!("\nAndroid setup looks complete.");
        Ok(())
    } else {
        Err(format!(
            "{} Android setup problem(s) found",
            report.failures
        ))
    }
}

fn dir_from_env(var: &str) -> Option<PathBuf> {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn check_sdk(report: &mut Report) -> Option<PathBuf> {
    let Some(sdk) = dir_from_env("ANDROID_HOME") else {
        report.fail(
            "ANDROID_HOME",
            "not set",
            "export ANDROID_HOME=\"$HOME/Android/Sdk\" (or wherever the SDK is installed)",
        );
        return None;
    };
    if !sdk.is_dir() {
        report.fail(
            "ANDROID_HOME",
            &format!("{} is not a directory", sdk.display()),
            "point ANDROID_HOME at the Android SDK directory",
        );
        return None;
    }
    report.ok("ANDROID_HOME", &sdk.display().to_string());

    let adb = sdk.join("platform-tools").join(exe("adb"));
    if adb.is_file() {
        report.ok("platform-tools", &adb.display().to_string());
    } else {
        report.fail(
            "platform-tools",
            &format!("{} not found", adb.display()),
            "install \"Android SDK Platform-Tools\" from Android Studio's SDK Manager",
        );
    }
    Some(sdk)
}

fn check_ndk(report: &mut Report) {
    let Some(ndk) = dir_from_env("NDK_HOME") else {
        report.fail(
            "NDK_HOME",
            "not set",
            "export NDK_HOME=\"$ANDROID_HOME/ndk/<version>\" (see `ls $ANDROID_HOME/ndk`)",
        );
        return;
    };
    match fs::read_to_string(ndk.join("source.properties")) {
        Ok(props) => {
            let version = props
                .lines()
                .find_map(|l| l.strip_prefix("Pkg.Revision = "))
                .unwrap_or("unknown version");
            report.ok("NDK_HOME", &format!("{} ({version})", ndk.display()));
        }
        Err(_) => report.fail(
            "NDK_HOME",
            &format!("{} does not look like an NDK", ndk.display()),
            "install \"NDK (Side by side)\" from Android Studio's SDK Manager and point NDK_HOME at it",
        ),
    }
}

fn check_java(report: &mut Report) {
    let Some(java_home) = dir_from_env("JAVA_HOME") else {
        report.fail(
            "JAVA_HOME",
            "not set",
            "export JAVA_HOME=/opt/android-studio/jbr (the JDK bundled with Android Studio)",
        );
        return;
    };
    let java = java_home.join("bin").join(exe("java"));
    let output = match Command::new(&java).arg("-version").output() {
        Ok(output) => output,
        Err(e) => {
            report.fail(
                "JAVA_HOME",
                &format!("could not run {}: {e}", java.display()),
                "point JAVA_HOME at a JDK directory containing bin/java",
            );
            return;
        }
    };
    // `java -version` prints to stderr, e.g. `openjdk version "25.0.3" 2026-04-21`.
    let text = String::from_utf8_lossy(&output.stderr);
    let first = text.lines().next().unwrap_or_default();
    let major = first
        .split('"')
        .nth(1)
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse::<u32>().ok());
    match major {
        Some(m) if m >= MIN_JAVA_MAJOR => report.ok("JAVA_HOME", first),
        Some(m) => report.fail(
            "JAVA_HOME",
            &format!("Java {m} is too old"),
            &format!("use JDK {MIN_JAVA_MAJOR} or newer, e.g. /opt/android-studio/jbr"),
        ),
        None => report.warn(
            "JAVA_HOME",
            &format!("could not read the Java version from `{first}`"),
            "check that JAVA_HOME points at a JDK",
        ),
    }
}

fn check_rust_target(report: &mut Report) {
    // Run in the repo so rustup uses the toolchain pinned in rust-toolchain.toml.
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(repo_root())
        .output();
    match output {
        Ok(o)
            if String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim() == RUST_TARGET) =>
        {
            report.ok("Rust target", RUST_TARGET);
        }
        Ok(_) => report.fail(
            "Rust target",
            &format!("{RUST_TARGET} not installed for the pinned toolchain"),
            &format!("rustup target add {RUST_TARGET} (run inside the repo)"),
        ),
        Err(e) => report.fail(
            "Rust target",
            &format!("could not run rustup: {e}"),
            "install rustup",
        ),
    }
}

fn check_platform(report: &mut Report, sdk: &Path) {
    let gradle = repo_root().join("apps/native/src-tauri/gen/android/app/build.gradle.kts");
    let Some(compile_sdk) = fs::read_to_string(&gradle).ok().and_then(|s| {
        s.lines()
            .find_map(|l| l.trim().strip_prefix("compileSdk = ").map(str::to_owned))
    }) else {
        report.warn(
            "SDK platform",
            &format!("could not read compileSdk from {}", gradle.display()),
            "check the generated Android project",
        );
        return;
    };
    let platforms = sdk.join("platforms");
    let installed = fs::read_dir(&platforms)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name == format!("android-{compile_sdk}")
                || name.starts_with(&format!("android-{compile_sdk}."))
        });
    if installed {
        report.ok("SDK platform", &format!("android-{compile_sdk}"));
    } else {
        report.fail(
            "SDK platform",
            &format!(
                "android-{compile_sdk} (compileSdk) not installed in {}",
                platforms.display()
            ),
            &format!(
                "install \"Android SDK Platform {compile_sdk}\" from Android Studio's SDK Manager"
            ),
        );
    }
}

/// A missing device is only a warning: building an APK does not need one.
fn check_device(report: &Report, sdk: &Path) {
    let adb = sdk.join("platform-tools").join(exe("adb"));
    let Ok(output) = Command::new(&adb).arg("devices").output() else {
        return; // Already reported by check_sdk.
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let devices: Vec<(&str, &str)> = text
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once('\t'))
        .collect();
    let ready: Vec<&str> = devices
        .iter()
        .filter(|(_, s)| *s == "device")
        .map(|(id, _)| *id)
        .collect();
    if !ready.is_empty() {
        report.ok("device", &format!("connected: {}", ready.join(", ")));
    } else if devices.iter().any(|(_, s)| *s == "unauthorized") {
        report.warn(
            "device",
            "connected but unauthorised",
            "unlock the phone and accept the \"Allow USB debugging\" prompt",
        );
    } else {
        report.warn(
            "device",
            "no device connected (only needed to install and run)",
            "connect the phone by USB with USB debugging enabled",
        );
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}
