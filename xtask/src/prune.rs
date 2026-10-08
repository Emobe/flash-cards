//! `cargo xtask prune [--days N] [--yes]`: deletes build output in `target/` that nothing needs.
//!
//! Cargo never removes old artifacts, so `target/` grows with every change to the code or the
//! features. This lists, and with `--yes` deletes:
//!
//! - **Scratch folders**: `target/scratch-*` and `target/cdp`, which hold experiments and browser
//!   profiles, not build output.
//! - **Incremental caches**: entries of `<profile>/incremental` untouched for `--days` days
//!   (default 7). Cargo recreates them on the next build.
//! - **Stale builds**: in each `<profile>/deps`, every build of a crate except the newest one of
//!   its kind (library, check-only library, executable). Crates that appear in `Cargo.lock` with
//!   more than one version are left alone, since their builds share a name but are all in use.
//!
//! Anything deleted is rebuilt by Cargo when needed; the worst case is a slower next build. It only
//! looks inside `target/` and does not touch `~/.cargo`. Do not run it while a build is running.
//! The default is a dry run.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::repo_root;

const USAGE: &str = "usage: cargo xtask prune [--days N] [--yes]";
const DEFAULT_DAYS: u64 = 7;

fn days(n: u64) -> Duration {
    Duration::from_secs(n * 24 * 60 * 60)
}

/// Top-level folders of `target/` that are not build output.
const SCRATCH_PREFIX: &str = "scratch-";
const SCRATCH_NAMES: &[&str] = &["cdp"];

#[derive(Debug)]
struct Options {
    days: u64,
    yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    Scratch,
    Incremental,
    Stale,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Scratch => "Scratch folders",
            Category::Incremental => "Incremental caches",
            Category::Stale => "Stale builds",
        }
    }
}

/// One thing to delete. `group` is what the report sums it under (a build folder, or the scratch
/// folder itself).
#[derive(Debug)]
struct Item {
    category: Category,
    group: PathBuf,
    path: PathBuf,
    bytes: u64,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let options = parse_args(args)?;
    let target = repo_root().join("target");
    if !target.is_dir() {
        println!("There is no target/ folder, so there is nothing to prune.");
        return Ok(());
    }
    let lock = fs::read_to_string(repo_root().join("Cargo.lock"))
        .map_err(|e| format!("reading Cargo.lock: {e}"))?;
    let cutoff = SystemTime::now()
        .checked_sub(days(options.days))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let items = plan(&target, &multi_version_names(&lock), cutoff)?;
    report(&target, &items);
    if items.is_empty() {
        return Ok(());
    }
    if !options.yes {
        println!("\nDry run: nothing was deleted. Run `cargo xtask prune --yes` to delete these.");
        return Ok(());
    }
    apply(&items)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        days: DEFAULT_DAYS,
        yes: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--yes" => options.yes = true,
            "--days" => {
                options.days = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| format!("--days needs a whole number. {USAGE}"))?;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(options)
}

/// Crate names (with `-` as `_`, the way rustc names its files) that `Cargo.lock` lists more than
/// once, which means several versions of that crate are built side by side.
fn multi_version_names(lock: &str) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut repeated = HashSet::new();
    for line in lock.lines() {
        if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|rest| rest.strip_suffix('"'))
        {
            let name = name.replace('-', "_");
            if !seen.insert(name.clone()) {
                repeated.insert(name);
            }
        }
    }
    repeated
}

/// Works out what to delete. Reads the file system, changes nothing.
fn plan(
    target: &Path,
    multi_version: &HashSet<String>,
    incremental_cutoff: SystemTime,
) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for entry in read_dir(target)? {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(SCRATCH_PREFIX) || SCRATCH_NAMES.contains(&name.as_str()) {
            items.push(Item {
                category: Category::Scratch,
                group: path.clone(),
                bytes: dir_size(&path),
                path,
            });
            continue;
        }
        // `target/debug` and friends, and `target/<triple>/debug` for cross builds.
        let mut build_dirs = vec![path.clone()];
        build_dirs.extend(
            read_dir(&path)?
                .into_iter()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path()),
        );
        for dir in build_dirs.into_iter().filter(|d| d.join("deps").is_dir()) {
            plan_incremental(&dir, incremental_cutoff, &mut items)?;
            plan_stale_builds(&dir, multi_version, &mut items)?;
        }
    }
    Ok(items)
}

fn plan_incremental(
    build_dir: &Path,
    cutoff: SystemTime,
    items: &mut Vec<Item>,
) -> Result<(), String> {
    let incremental = build_dir.join("incremental");
    if !incremental.is_dir() {
        return Ok(());
    }
    for entry in read_dir(&incremental)? {
        let path = entry.path();
        // A crate's folder changes whenever a build adds or removes a session in it.
        let touched = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| SystemTime::now());
        if touched < cutoff {
            items.push(Item {
                category: Category::Incremental,
                group: build_dir.to_owned(),
                bytes: dir_size(&path),
                path,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum UnitKind {
    /// Has a compiled library (`.rlib`, `.so`, `.a`, `.dll`).
    Library,
    /// Only metadata (`cargo check` and clippy).
    Check,
    /// A program or a test program.
    Executable,
    /// Anything else (loose `.d` files and so on): never deleted.
    Other,
}

/// All the files one compilation left in `deps`, told apart by the hash in their names.
struct Unit {
    stem: String,
    kind: UnitKind,
    newest: SystemTime,
    files: Vec<(PathBuf, u64)>,
}

fn plan_stale_builds(
    build_dir: &Path,
    multi_version: &HashSet<String>,
    items: &mut Vec<Item>,
) -> Result<(), String> {
    // `Other` until a file shows what the unit is; `d` files only supply a fallback name.
    let mut units: HashMap<String, Unit> = HashMap::new();
    for entry in read_dir(&build_dir.join("deps"))? {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((stem, hash, ext)) = parse_name(&name) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());
        let unit = units.entry(hash).or_insert_with(|| Unit {
            stem: String::new(),
            kind: UnitKind::Other,
            newest: SystemTime::UNIX_EPOCH,
            files: Vec::new(),
        });
        unit.newest = unit.newest.max(modified);
        unit.files.push((entry.path(), meta.len()));
        let kind = match ext.as_str() {
            "rlib" | "so" | "a" | "dll" | "dylib" | "lib" | "dll.lib" => UnitKind::Library,
            "rmeta" => UnitKind::Check,
            "" | "exe" | "wasm" => UnitKind::Executable,
            _ => UnitKind::Other,
        };
        // A unit with a library is a library even though its `rmeta` also shows up.
        if kind != UnitKind::Other && (unit.kind == UnitKind::Other || kind == UnitKind::Library) {
            unit.kind = kind;
        }
        if kind != UnitKind::Other || unit.stem.is_empty() {
            unit.stem = stem;
        }
    }

    let mut groups: HashMap<(UnitKind, String), Vec<Unit>> = HashMap::new();
    for unit in units.into_values() {
        if unit.kind == UnitKind::Other {
            continue;
        }
        let stem = match unit.kind {
            UnitKind::Executable => unit.stem.clone(),
            _ => unit
                .stem
                .strip_prefix("lib")
                .unwrap_or(&unit.stem)
                .to_owned(),
        };
        if multi_version.contains(&stem) {
            continue;
        }
        groups.entry((unit.kind, stem)).or_default().push(unit);
    }
    for mut group in groups.into_values() {
        group.sort_by_key(|unit| unit.newest);
        group.pop(); // the newest build stays
        for unit in group {
            for (path, bytes) in unit.files {
                items.push(Item {
                    category: Category::Stale,
                    group: build_dir.to_owned(),
                    path,
                    bytes,
                });
            }
        }
    }
    Ok(())
}

/// Splits a `deps` file name like `libfc_core-0123456789abcdef.rlib` into the name, the
/// 16-digit hash and the extension (`rlib`, empty for a program). `None` for any other name.
fn parse_name(name: &str) -> Option<(String, String, String)> {
    let (base, ext) = name.split_once('.').unwrap_or((name, ""));
    let (stem, hash) = base.rsplit_once('-')?;
    let is_hash = hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    (is_hash && !stem.is_empty()).then(|| (stem.to_owned(), hash.to_owned(), ext.to_owned()))
}

fn report(target: &Path, items: &[Item]) {
    if items.is_empty() {
        println!("Nothing to prune in {}.", target.display());
        return;
    }
    let mut total = 0;
    for category in [Category::Scratch, Category::Incremental, Category::Stale] {
        let mut groups: Vec<(&Path, usize, u64)> = Vec::new();
        for item in items.iter().filter(|i| i.category == category) {
            match groups.iter_mut().find(|(group, ..)| *group == item.group) {
                Some((_, count, bytes)) => {
                    *count += 1;
                    *bytes += item.bytes;
                }
                None => groups.push((&item.group, 1, item.bytes)),
            }
        }
        if groups.is_empty() {
            continue;
        }
        let subtotal: u64 = groups.iter().map(|(.., bytes)| bytes).sum();
        total += subtotal;
        println!("\n{}: {}", category.label(), format_size(subtotal));
        groups.sort();
        for (group, count, bytes) in groups {
            let shown = group.strip_prefix(target).unwrap_or(group);
            let what = if category == Category::Scratch {
                String::new()
            } else {
                format!(", {count} {}", if count == 1 { "item" } else { "items" })
            };
            println!("  {}  {}{what}", format_size(bytes), shown.display());
        }
    }
    println!("\nTotal: {}", format_size(total));
}

/// Deletes the planned items. A path already gone counts as deleted.
fn apply(items: &[Item]) -> Result<(), String> {
    let mut freed = 0;
    let mut failures = Vec::new();
    for item in items {
        let is_dir = fs::symlink_metadata(&item.path).is_ok_and(|m| m.is_dir());
        let result = if is_dir {
            fs::remove_dir_all(&item.path)
        } else {
            fs::remove_file(&item.path)
        };
        match result {
            Ok(()) => freed += item.bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => failures.push(format!("  {}: {e}", item.path.display())),
        }
    }
    println!("\nDeleted {}.", format_size(freed));
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "could not delete {} items:\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

fn read_dir(dir: &Path) -> Result<Vec<fs::DirEntry>, String> {
    fs::read_dir(dir)
        .and_then(|entries| entries.collect())
        .map_err(|e| format!("reading {}: {e}", dir.display()))
}

/// Size of a folder in bytes, not following symbolic links. Unreadable parts count as 0.
fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path)
        .map(|entries| entries.flatten().map(|entry| dir_size(&entry.path())).sum())
        .unwrap_or(0)
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder under the system temp directory, removed on drop.
    struct Fake {
        root: PathBuf,
    }

    impl Fake {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "fc-prune-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Fake { root }
        }

        /// Makes `target/<relative>` with `age_days` as its modified time.
        fn file(&self, relative: &str, age_days: u64) -> PathBuf {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let file = File::create(&path).unwrap();
            file.set_len(10).unwrap();
            file.set_modified(SystemTime::now() - days(age_days))
                .unwrap();
            path
        }

        fn planned(&self, multi: &[&str]) -> Vec<String> {
            let multi = multi.iter().map(|s| (*s).to_owned()).collect();
            // A cutoff in the past keeps every incremental folder.
            let mut names: Vec<String> = plan(&self.root, &multi, SystemTime::UNIX_EPOCH)
                .unwrap()
                .iter()
                .map(|i| {
                    i.path
                        .strip_prefix(&self.root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    const OLD: &str = "aaaaaaaaaaaaaaaa";
    const NEW: &str = "bbbbbbbbbbbbbbbb";

    #[test]
    fn names_split_into_stem_hash_and_extension() {
        assert_eq!(
            parse_name("libfc_core-0123456789abcdef.rlib"),
            Some((
                "libfc_core".into(),
                "0123456789abcdef".into(),
                "rlib".into()
            ))
        );
        assert_eq!(
            parse_name("fc-cli-0123456789abcdef"),
            Some(("fc-cli".into(), "0123456789abcdef".into(), String::new()))
        );
        assert_eq!(parse_name("fc_core.d"), None);
        assert_eq!(parse_name("libfoo-xyz.rlib"), None);
        assert_eq!(parse_name("-0123456789abcdef.rlib"), None);
    }

    #[test]
    fn older_builds_of_a_library_go_and_the_newest_stays() {
        let fake = Fake::new();
        for ext in ["rlib", "rmeta", "d"] {
            let stem = if ext == "d" { "foo" } else { "libfoo" };
            fake.file(&format!("debug/deps/{stem}-{OLD}.{ext}"), 5);
            fake.file(&format!("debug/deps/{stem}-{NEW}.{ext}"), 1);
        }
        assert_eq!(
            fake.planned(&[]),
            [
                format!("debug/deps/foo-{OLD}.d"),
                format!("debug/deps/libfoo-{OLD}.rlib"),
                format!("debug/deps/libfoo-{OLD}.rmeta"),
            ]
        );
    }

    #[test]
    fn a_newer_check_build_does_not_evict_the_compiled_library() {
        let fake = Fake::new();
        fake.file(&format!("debug/deps/libfoo-{OLD}.rlib"), 5);
        fake.file(&format!("debug/deps/libfoo-{OLD}.rmeta"), 5);
        fake.file(&format!("debug/deps/libfoo-{NEW}.rmeta"), 1);
        assert_eq!(fake.planned(&[]), Vec::<String>::new());
    }

    #[test]
    fn a_library_and_a_program_of_the_same_name_are_kept_apart() {
        let fake = Fake::new();
        fake.file(&format!("debug/deps/libfoo-{OLD}.rlib"), 5);
        fake.file(&format!("debug/deps/foo-{NEW}"), 1);
        assert_eq!(fake.planned(&[]), Vec::<String>::new());
    }

    #[test]
    fn older_test_programs_go() {
        let fake = Fake::new();
        fake.file(&format!("debug/deps/foo-{OLD}"), 5);
        fake.file(&format!("debug/deps/foo-{NEW}"), 1);
        assert_eq!(fake.planned(&[]), [format!("debug/deps/foo-{OLD}")]);
    }

    #[test]
    fn a_crate_built_in_several_versions_is_left_alone() {
        let fake = Fake::new();
        fake.file(&format!("debug/deps/libsyn-{OLD}.rlib"), 30);
        fake.file(&format!("debug/deps/libsyn-{NEW}.rlib"), 1);
        assert_eq!(fake.planned(&["syn"]), Vec::<String>::new());
        assert_eq!(fake.planned(&[]), [format!("debug/deps/libsyn-{OLD}.rlib")]);
    }

    #[test]
    fn files_that_are_not_builds_stay() {
        let fake = Fake::new();
        fake.file("debug/deps/README", 90);
        fake.file(&format!("debug/deps/foo-{OLD}.d"), 90);
        fake.file(&format!("debug/deps/foo-{NEW}.d"), 1);
        assert_eq!(fake.planned(&[]), Vec::<String>::new());
    }

    #[test]
    fn cross_builds_are_pruned_per_target() {
        let fake = Fake::new();
        fake.file(
            &format!("aarch64-linux-android/debug/deps/libfoo-{OLD}.rlib"),
            5,
        );
        fake.file(
            &format!("aarch64-linux-android/debug/deps/libfoo-{NEW}.rlib"),
            1,
        );
        // The host build of the same crate is a separate folder and keeps its own newest.
        fake.file(&format!("debug/deps/libfoo-{OLD}.rlib"), 9);
        assert_eq!(
            fake.planned(&[]),
            [format!(
                "aarch64-linux-android/debug/deps/libfoo-{OLD}.rlib"
            )]
        );
    }

    #[test]
    fn scratch_folders_go_and_other_folders_stay() {
        let fake = Fake::new();
        fake.file("scratch-2.3a/profile-1/data", 1);
        fake.file("cdp/profile-2/data", 1);
        fake.file("other/data", 90);
        fake.file("debug/fc", 90);
        assert_eq!(fake.planned(&[]), ["cdp", "scratch-2.3a"]);
    }

    #[test]
    fn incremental_folders_follow_the_cutoff() {
        let fake = Fake::new();
        fake.file("debug/deps/placeholder", 0);
        fake.file("debug/incremental/foo-1/session/data", 0);
        let multi = HashSet::new();
        let past = SystemTime::now() - days(1);
        let future = SystemTime::now() + days(1);
        let kept = plan(&fake.root, &multi, past).unwrap();
        let removed = plan(&fake.root, &multi, future).unwrap();
        assert!(kept.is_empty());
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].category, Category::Incremental);
        assert!(removed[0].path.ends_with("incremental/foo-1"));
    }

    #[test]
    fn planning_changes_nothing_and_apply_deletes_only_the_plan() {
        let fake = Fake::new();
        let stale = fake.file(&format!("debug/deps/foo-{OLD}"), 5);
        let current = fake.file(&format!("debug/deps/foo-{NEW}"), 1);
        let scratch = fake.file("scratch-x/data", 1);
        let items = plan(&fake.root, &HashSet::new(), SystemTime::UNIX_EPOCH).unwrap();
        assert!(stale.exists() && current.exists() && scratch.exists());
        apply(&items).unwrap();
        assert!(!stale.exists());
        assert!(!scratch.exists());
        assert!(current.exists());
    }

    #[test]
    fn crates_listed_twice_in_the_lockfile_are_found() {
        let lock = "[[package]]\nname = \"syn\"\nversion = \"1.0.0\"\n\n\
                    [[package]]\nname = \"syn\"\nversion = \"2.0.0\"\n\n\
                    [[package]]\nname = \"fc-core\"\nversion = \"0.1.0\"\ndependencies = [\n \"syn 2.0.0\",\n]\n";
        let repeated = multi_version_names(lock);
        assert!(repeated.contains("syn"));
        assert!(!repeated.contains("fc_core"));
    }

    #[test]
    fn arguments_are_checked() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let options = parse_args(&args(&["--days", "0", "--yes"])).unwrap();
        assert_eq!((options.days, options.yes), (0, true));
        let options = parse_args(&args(&[])).unwrap();
        assert_eq!((options.days, options.yes), (DEFAULT_DAYS, false));
        assert!(parse_args(&args(&["--days"])).is_err());
        assert!(parse_args(&args(&["--days", "soon"])).is_err());
        assert!(parse_args(&args(&["--force"])).is_err());
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }
}
