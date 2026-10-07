//! Every public operation of `fc_core::Collection` has a command in `fc`, or a written reason why
//! it has none. A new operation fails this test until someone decides which.
//!
//! The operations are read from the source of `fc-core`: each `pub fn` in an `impl Collection`
//! block (code formatted by rustfmt, so the block starts and ends at column 0).

mod common;
use common::*;

/// The command that covers an operation, or why it has none.
enum Covered {
    By(&'static str),
    Not(&'static str),
}
use Covered::{By, Not};

const SYNC_PLUMBING: &str =
    "sync plumbing for the merge (ADR 0008): only `merge` and the sync code call it";

#[rustfmt::skip]
const OPERATIONS: &[(&str, Covered)] = &[
    ("add_field", By("field")), ("add_media", By("add-media")), ("add_note", By("add-note")),
    ("add_note_to_deck", By("add-note")), ("add_tags", By("tag")), ("add_template", By("template")),
    ("answer", By("answer")), ("backup_settings", By("backup-settings")),
    ("bury_cards", By("bury")), ("card_events", By("schedule")), ("card_history", By("history")),
    ("card_schedule", By("schedule")), ("cards_of_note", By("notes")), ("changes", By("merge")),
    ("check_media", By("media-check")), ("close", Not("every command closes the collection it opened")),
    ("create", By("new")), ("create_deck", By("add-deck")), ("create_note_type", By("notetype")),
    ("create_preset", By("preset")), ("daily_counts", By("stats")),
    ("day_start_hour", By("day-start-hour")), ("deck", By("decks")), ("deck_counts", By("due")),
    ("deck_preset", By("decks")), ("decks", By("decks")),
    ("deleted_cards_of_note", By("notes")), ("deleted_decks", By("decks")),
    ("deleted_notes", By("notes")), ("deleted_note_types", By("notetypes")),
    ("deleted_presets", By("decks")), ("delete_deck", By("deck")), ("delete_media", By("delete-media")),
    ("delete_note", By("delete-note")), ("delete_note_type", By("notetype")),
    ("delete_preset", By("preset")), ("delete_saved_search", By("delete-search")),
    ("delete_tag", By("delete-tag")), ("delete_unused_media", By("delete-unused-media")),
    ("device_id", By("device-id")), ("due_forecast", By("forecast")),
    ("end_study_session", Not("a study session lives in one process and only emits events (ADR 0009); one command per process has nothing to emit them to")),
    ("export_backup", By("export")), ("find_duplicates", By("find-duplicates")),
    ("hlc", Not(SYNC_PLUMBING)), ("import_backup", By("import")), ("info", By("info")),
    ("knows_register", Not(SYNC_PLUMBING)),
    ("listen", Not("a Rust hook for events (docs/events.md); a command has nobody to listen")),
    ("media_bytes", By("media-get")), ("media_file", By("media")), ("media_files", By("media")),
    ("media_references", By("media")), ("merge", By("merge")), ("move_cards", By("move-cards")),
    ("move_deck", By("deck")), ("move_field", By("field")), ("move_template", By("template")),
    ("next_card", By("next")), ("note", By("notes")), ("notes", By("notes")),
    ("notes_with_tag", By("tagged")), ("note_tags", By("notes")), ("note_type", By("notetypes")),
    ("note_types", By("notetypes")), ("observe_hlc", Not(SYNC_PLUMBING)), ("open", Not("every command opens its collection")),
    ("open_or_create", Not("`new` creates and every other command opens; this is for the apps")),
    ("optimisation_data", By("optimise")), ("optimise_preset", By("optimise")),
    ("pending_migration", By("info")), ("preset", By("decks")), ("presets", By("decks")),
    ("rebuild_schedule", By("rebuild-schedule")), ("regenerate_device_id", By("device-id")),
    ("register_clock", Not(SYNC_PLUMBING)), ("removed_fields", By("notetypes")),
    ("removed_templates", By("notetypes")), ("remove_field", By("field")),
    ("remove_tags", By("untag")), ("remove_template", By("template")), ("rename_deck", By("deck")),
    ("rename_field", By("field")), ("rename_note_type", By("notetype")),
    ("rename_preset", By("preset")), ("rename_tag", By("rename-tag")),
    ("rename_template", By("template")), ("render_card", By("render")),
    ("require_feature", Not(SYNC_PLUMBING)), ("restore_backup", By("restore")),
    ("restore_deck", By("deck")), ("restore_field", By("field")), ("restore_media", By("restore-media")),
    ("restore_note", By("restore-note")), ("restore_note_type", By("notetype")),
    ("restore_preset", By("preset")), ("restore_template", By("template")),
    ("retention", By("stats")), ("run_saved_search", By("run-search")),
    ("saved_search", By("searches")), ("saved_searches", By("searches")),
    ("save_search", By("save-search")), ("search", By("search")),
    ("set_backup_error", Not("written by the app's automatic backup when it fails; `backup-settings` shows it")),
    ("set_backup_settings", By("backup-settings")), ("set_day_start_hour", By("day-start-hour")),
    ("set_deck_limits_include_subdecks", By("deck")), ("set_deck_preset", By("preset")),
    ("set_note_fields", By("edit-note")), ("set_note_tags", By("set-tags")),
    ("set_note_type_css", By("notetype")), ("set_preset_options", By("preset")),
    ("set_sort_field", By("notetype")), ("set_template_text", By("template")),
    ("start_study_session", Not("see end_study_session")), ("store_unknown_register", Not(SYNC_PLUMBING)),
    ("study_counts", By("due")), ("study_today", By("stats")), ("suspend_cards", By("suspend")),
    ("tags", By("tags")), ("unbury_cards", By("unbury")), ("unbury_deck", By("unbury")),
    ("undo_answer", By("undo")), ("unknown_registers", Not(SYNC_PLUMBING)),
    ("unsupported_features", By("info")), ("unsuspend_cards", By("unsuspend")),
    ("update_saved_search", By("update-search")),
    ("write", Not("the one way to write a synced table: every command that changes data goes through it")),
];

/// The names of the `pub fn`s in the `impl Collection` blocks of one source file.
fn operations_in(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut inside = false;
    for line in source.lines() {
        if line.starts_with("impl") && line.trim_end().ends_with(" Collection {") {
            inside = true;
        } else if line.starts_with('}') {
            inside = false;
        } else if inside && let Some(rest) = line.strip_prefix("    pub fn ") {
            let end = rest.find(['(', '<']).unwrap();
            found.push(rest[..end].to_owned());
        }
    }
    found
}

fn rust_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if path.is_dir() {
            rust_files(&path, files);
        } else if name.ends_with(".rs") && !name.ends_with("tests.rs") {
            files.push(path);
        }
    }
}

#[test]
fn every_public_operation_has_a_command_or_a_reason() {
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fc-core/src");
    let mut files = Vec::new();
    rust_files(&core, &mut files);
    let mut found: Vec<String> = files
        .iter()
        .flat_map(|path| operations_in(&std::fs::read_to_string(path).unwrap()))
        .collect();
    found.sort();
    found.dedup();
    for (name, covered) in OPERATIONS {
        if let Not(reason) = covered {
            assert!(!reason.is_empty(), "{name} needs a reason");
        }
    }
    assert!(found.len() > 100, "the scan found only {}", found.len());

    let listed: Vec<&str> = OPERATIONS.iter().map(|(name, _)| *name).collect();
    let new: Vec<&String> = found
        .iter()
        .filter(|n| !listed.contains(&n.as_str()))
        .collect();
    assert!(
        new.is_empty(),
        "These public Collection operations are not in OPERATIONS in tests/coverage.rs. Give each \
         a command (and add it there) or write why it has none: {new:?}"
    );
    let gone: Vec<&&str> = listed
        .iter()
        .filter(|n| !found.iter().any(|f| f == **n))
        .collect();
    assert!(
        gone.is_empty(),
        "These are in OPERATIONS but no longer in fc-core: {gone:?}"
    );
}

#[test]
fn every_command_in_the_table_is_in_the_help_and_the_dispatcher() {
    let help = out(&fc(&["help"]));
    let mut commands: Vec<&str> = OPERATIONS
        .iter()
        .filter_map(|(_, covered)| match covered {
            By(command) => Some(*command),
            Not(_) => None,
        })
        .collect();
    commands.sort();
    commands.dedup();
    for command in commands {
        assert!(
            help.lines()
                .any(|l| l.starts_with(&format!("  fc {command} "))),
            "`fc help` has no line for \"{command}\""
        );
        // With no arguments a known command says the arguments are wrong, never "unknown".
        let message = err(&fc(&[command]));
        assert!(
            !message.contains("Unknown command"),
            "\"{command}\": {message}"
        );
    }
}

#[test]
fn every_command_in_the_help_is_known_to_the_dispatcher() {
    let help = out(&fc(&["help"]));
    let mut commands: Vec<&str> = help
        .lines()
        .filter_map(|l| l.strip_prefix("  fc "))
        .filter_map(|l| l.split(' ').next())
        .collect();
    commands.sort();
    commands.dedup();
    assert!(commands.len() > 50, "found only {}", commands.len());
    for command in commands.into_iter().filter(|c| *c != "help") {
        let message = err(&fc(&[command]));
        assert!(
            !message.contains("Unknown command"),
            "\"{command}\": {message}"
        );
    }
}
