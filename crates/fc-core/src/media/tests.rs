use std::sync::Arc;

use super::name::{digits_in, hash_of, id_of};
use super::*;
use crate::clock::{Host, ManualClock};
use crate::collection::{Collection, MIGRATIONS};
use crate::notetype::builtin;
use crate::sync::{SYNCED_TABLES, check_schema};

const START: i64 = 1_700_000_000_000;

fn host(clock: &Arc<ManualClock>, installation: &[u8; 16]) -> Host {
    Host {
        clock: clock.clone(),
        installation_id: Id::from_bytes(*installation),
    }
}

fn collection() -> Collection {
    let clock = Arc::new(ManualClock::new(START));
    Collection::create(":memory:", host(&clock, b"installation-one")).unwrap()
}

fn two_collections() -> (Collection, Collection) {
    let clock = Arc::new(ManualClock::new(START));
    (
        Collection::create(":memory:", host(&clock, b"installation-one")).unwrap(),
        Collection::create(":memory:", host(&clock, b"installation-two")).unwrap(),
    )
}

const PNG: &[u8] = b"\x89PNG not really an image";
const WAV: &[u8] = b"RIFF not really audio";

/// A basic note with these two fields. Returns the note ID.
fn note(c: &Collection, front: &str, back: &str) -> Id {
    let note_type = c.note_type(builtin::basic()).unwrap().unwrap();
    let f: Vec<Id> = note_type.fields.iter().map(|f| f.id).collect();
    c.add_note(builtin::basic(), &[(f[0], front), (f[1], back)])
        .unwrap()
        .id
}

fn img(name: &str) -> String {
    format!("<img src=\"{name}\">")
}

// --- adding ---------------------------------------------------------------------------------

#[test]
fn adding_a_file_gives_a_name_that_carries_its_hash() {
    let c = collection();
    let added = c.add_media("cat.png", PNG).unwrap();
    let hash = hash_of(PNG);
    assert_eq!(hash.len(), 64);
    assert_eq!(added.name, format!("cat-{}.png", &hash[..HASH_DIGITS]));
    assert!(added.new);
    assert_eq!(added.file.hash, hash);
    assert_eq!(added.file.size, PNG.len() as u64);
    assert!(added.file.has_bytes);
    assert!(!added.file.deleted);
    assert_eq!(added.file.id, id_of(&hash));
    assert_eq!(c.media_bytes(&added.name).unwrap(), PNG);
}

#[test]
fn the_hash_is_sha_256() {
    // The well-known digest of "abc".
    assert_eq!(
        hash_of(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn identical_files_are_stored_once_whatever_they_are_called() {
    let c = collection();
    let a = c.add_media("cat.png", PNG).unwrap();
    let b = c
        .add_media("/home/me/Pictures/other name.PNG", PNG)
        .unwrap();
    assert!(a.new);
    assert!(!b.new);
    assert_ne!(a.name, b.name);
    assert_eq!(a.file, b.file);
    let files = c.media_files().unwrap();
    assert_eq!(files.len(), 1);
    let blobs: i64 = c
        .conn
        .query_row("SELECT count(*) FROM media_blob", [], |r| r.get(0))
        .unwrap();
    assert_eq!(blobs, 1);
    // Both names reach the same bytes.
    assert_eq!(c.media_bytes(&a.name).unwrap(), PNG);
    assert_eq!(c.media_bytes(&b.name).unwrap(), PNG);
}

#[test]
fn adding_the_same_file_again_writes_nothing() {
    let c = collection();
    let first = c.add_media("cat.png", PNG).unwrap();
    let id = first.file.id;
    let before = c.register_clock("media_file", id, "deleted").unwrap();
    assert!(before.is_some());
    c.add_media("cat.png", PNG).unwrap();
    assert_eq!(
        c.register_clock("media_file", id, "deleted").unwrap(),
        before
    );
}

#[test]
fn different_files_with_the_same_name_get_different_names() {
    let c = collection();
    let a = c.add_media("image.jpg", b"one").unwrap();
    let b = c.add_media("image.jpg", b"two").unwrap();
    assert_ne!(a.name, b.name);
    assert_eq!(c.media_bytes(&a.name).unwrap(), b"one");
    assert_eq!(c.media_bytes(&b.name).unwrap(), b"two");
}

#[test]
fn an_empty_file_is_refused() {
    let c = collection();
    assert_eq!(c.add_media("a.png", b""), Err(MediaError::Empty));
    assert!(c.media_files().unwrap().is_empty());
    assert!(MediaError::Empty.to_string().contains("empty"));
}

#[test]
fn names_are_safe_in_html_sound_tags_and_css() {
    let hash = hash_of(b"x");
    let digits = &hash[..HASH_DIGITS];
    for (original, expected) in [
        ("cat.png", format!("cat-{digits}.png")),
        ("Cat.PNG", format!("Cat-{digits}.png")),
        ("a b\"c<d>&e'f].mp3", format!("a_b_c_d_e_f-{digits}.mp3")),
        ("../../etc/passwd", format!("passwd-{digits}")),
        ("C:\\Users\\me\\kot.jpg", format!("kot-{digits}.jpg")),
        ("", format!("file-{digits}")),
        (".png", format!("file-{digits}.png")),
        ("???.mp3", format!("file-{digits}.mp3")),
        ("zażółć gęślą.ogg", format!("zażółć_gęślą-{digits}.ogg")),
        ("kot.tar.gz", format!("kot_tar-{digits}.gz")),
        ("kot.p#n%g", format!("kot-{digits}.png")),
        ("kot.waytoolongextension", format!("kot-{digits}.waytoolo")),
        ("-_-.png", format!("file-{digits}.png")),
    ] {
        assert_eq!(file_name(original, &hash), expected, "{original}");
    }
}

#[test]
fn a_long_stem_is_cut_and_every_name_reads_back() {
    let hash = hash_of(b"x");
    let long = format!("{}.png", "é".repeat(500));
    let name = file_name(&long, &hash);
    assert_eq!(name.chars().count(), 40 + 1 + HASH_DIGITS + 4);
    assert_eq!(digits_in(&name).as_deref(), Some(&hash[..HASH_DIGITS]));
    for original in ["a.b.c", "x", "", " ", "日本語.jpg", "\u{0}.bin", "a\nb.txt"] {
        let name = file_name(original, &hash);
        assert!(
            name.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')),
            "{name:?}"
        );
        assert_eq!(
            digits_in(&name).as_deref(),
            Some(&hash[..HASH_DIGITS]),
            "{name:?}"
        );
    }
}

#[test]
fn only_names_in_the_format_belong_to_a_file() {
    let c = collection();
    let added = c.add_media("cat.png", PNG).unwrap();
    let digits = &added.file.hash[..HASH_DIGITS];
    // The stem is only a hint.
    assert!(
        c.media_file(&format!("anything-{digits}.png"))
            .unwrap()
            .is_some()
    );
    assert!(
        c.media_file(&format!("anything-{}.png", digits.to_uppercase()))
            .unwrap()
            .is_some()
    );
    for name in [
        "cat.png",
        "paste-123.jpg",
        "",
        "-",
        &format!("cat{digits}.png"),
        &format!("cat-{}.png", &digits[..15]),
        &format!("cat-{}0.png", &digits[..15]),
        &format!("cat-{digits}.png.bak.exe.txt.toolong"),
    ] {
        let found = c.media_file(name).unwrap();
        assert!(found.is_none(), "{name:?}");
        assert_eq!(
            c.media_bytes(name),
            Err(MediaError::NotFound(name.to_owned()))
        );
    }
}

#[test]
fn a_file_added_as_a_name_with_no_extension_works() {
    let c = collection();
    let added = c.add_media("README", b"text").unwrap();
    assert!(!added.name.contains('.'));
    assert_eq!(c.media_bytes(&added.name).unwrap(), b"text");
}

// --- references, unused and missing ----------------------------------------------------------

#[test]
fn a_file_named_in_a_field_is_used_and_one_that_is_not_is_unused() {
    let c = collection();
    let used = c.add_media("cat.png", PNG).unwrap();
    let unused = c.add_media("song.wav", WAV).unwrap();
    let n = note(&c, &img(&used.name), "back");
    let report = c.check_media().unwrap();
    assert_eq!(report.unused, vec![unused.file.clone()]);
    assert!(report.missing.is_empty());
    assert!(report.without_bytes.is_empty());
    let references = c.media_references().unwrap();
    assert_eq!(
        references,
        vec![Reference {
            name: used.name.clone(),
            notes: vec![n],
            note_types: vec![]
        }]
    );
}

#[test]
fn sound_tags_and_every_media_element_count_as_references() {
    let c = collection();
    let a = c.add_media("a.png", b"a").unwrap();
    let b = c.add_media("b.mp3", b"b").unwrap();
    let d = c.add_media("d.mp4", b"d").unwrap();
    let e = c.add_media("e.ogg", b"e").unwrap();
    note(
        &c,
        &format!("<IMG SRC='{}'> [sound:{}]", a.name, b.name),
        "x",
    );
    note(
        &c,
        &format!(
            "<video src=\"{}\"><source src=\"{}\"></video>",
            d.name, e.name
        ),
        "x",
    );
    let report = c.check_media().unwrap();
    assert!(report.unused.is_empty(), "{report:?}");
    assert!(report.missing.is_empty());
}

#[test]
fn a_missing_file_is_a_name_with_no_file() {
    let c = collection();
    let present = c.add_media("cat.png", PNG).unwrap();
    // The right format, but the file was never added here.
    let absent = file_name("dog.png", &hash_of(b"never added"));
    let n = note(
        &c,
        &format!(
            "{} {} <img src=\"paste-1.jpg\"> <img src=\"https://x.test/a.png\">",
            img(&present.name),
            img(&absent)
        ),
        "[sound:old name.mp3]",
    );
    let report = c.check_media().unwrap();
    let missing: Vec<&str> = report.missing.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        missing,
        vec![absent.as_str(), "old name.mp3", "paste-1.jpg"]
    );
    assert!(report.missing.iter().all(|r| r.notes == vec![n]));
    assert!(report.unused.is_empty());
}

#[test]
fn templates_and_styles_count_as_references() {
    let c = collection();
    let logo = c.add_media("logo.png", b"logo").unwrap();
    let font = c.add_media("face.woff2", b"font").unwrap();
    let bell = c.add_media("bell.mp3", b"bell").unwrap();
    let spare = c.add_media("spare.png", b"spare").unwrap();
    let note_type = c
        .create_note_type("Pictures", crate::notetype::Kind::Standard)
        .unwrap();
    c.add_field(note_type, "Word").unwrap();
    c.add_template(
        note_type,
        "Card",
        &format!(
            "{{{{Word}}}} {} <img src=\"{{{{Word}}}}\">",
            img(&logo.name)
        ),
        &format!("[sound:{}]", bell.name),
    )
    .unwrap();
    c.set_note_type_css(
        note_type,
        &format!("@font-face {{ font-family: F; src: url('{}') }}", font.name),
    )
    .unwrap();
    let report = c.check_media().unwrap();
    assert_eq!(report.unused, vec![spare.file.clone()]);
    // `<img src="{{Word}}">` is filled in later, so it is not a name.
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    let references = c.media_references().unwrap();
    let names: Vec<&str> = references.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names.len(), 3);
    assert!(
        references
            .iter()
            .all(|r| r.note_types == vec![note_type] && r.notes.is_empty())
    );
}

#[test]
fn a_removed_template_still_counts_because_it_can_come_back() {
    let c = collection();
    let logo = c.add_media("logo.png", b"logo").unwrap();
    let note_type = c
        .create_note_type("Pictures", crate::notetype::Kind::Standard)
        .unwrap();
    c.add_field(note_type, "Word").unwrap();
    c.add_template(note_type, "One", "{{Word}}", "x").unwrap();
    let two = c
        .add_template(note_type, "Two", &img(&logo.name), "x")
        .unwrap();
    c.remove_template(note_type, two).unwrap();
    assert!(c.check_media().unwrap().unused.is_empty());
}

#[test]
fn a_note_in_the_trash_still_names_its_file() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    let n = note(&c, &img(&cat.name), "back");
    c.delete_note(n).unwrap();
    assert!(c.check_media().unwrap().unused.is_empty());
    assert_eq!(c.delete_unused_media().unwrap(), vec![]);
    assert_eq!(
        c.delete_media(&cat.name),
        Err(MediaError::StillUsed(cat.name.clone()))
    );
}

#[test]
fn editing_a_note_so_it_stops_naming_a_file_makes_the_file_unused() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    let n = note(&c, &img(&cat.name), "back");
    assert!(c.check_media().unwrap().unused.is_empty());
    let note_type = c.note_type(builtin::basic()).unwrap().unwrap();
    c.set_note_fields(n, &[(note_type.fields[0].id, "kot")])
        .unwrap();
    assert_eq!(c.check_media().unwrap().unused, vec![cat.file]);
}

#[test]
fn text_that_only_looks_like_a_tag_names_nothing() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    note(
        &c,
        &format!("the file {} and 1 < 2 and [sound:", cat.name),
        "a < b",
    );
    assert_eq!(c.check_media().unwrap().unused.len(), 1);
    assert!(c.check_media().unwrap().missing.is_empty());
}

// --- deleting -------------------------------------------------------------------------------

#[test]
fn deleting_unused_files_keeps_the_used_ones_and_the_bytes() {
    let c = collection();
    let used = c.add_media("cat.png", PNG).unwrap();
    let unused = c.add_media("song.wav", WAV).unwrap();
    note(&c, &img(&used.name), "back");
    let deleted = c.delete_unused_media().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].id, unused.file.id);
    assert!(deleted[0].deleted);
    let files = c.media_files().unwrap();
    assert_eq!(files.len(), 2);
    assert!(
        files
            .iter()
            .find(|f| f.id == unused.file.id)
            .unwrap()
            .deleted
    );
    assert!(!files.iter().find(|f| f.id == used.file.id).unwrap().deleted);
    // A deleted file is not served, and it is no longer reported as unused.
    assert_eq!(
        c.media_bytes(&unused.name),
        Err(MediaError::NotFound(unused.name.clone()))
    );
    assert!(c.check_media().unwrap().unused.is_empty());
    // The bytes are still stored, so it can come back.
    let blobs: i64 = c
        .conn
        .query_row("SELECT count(*) FROM media_blob", [], |r| r.get(0))
        .unwrap();
    assert_eq!(blobs, 2);
    c.restore_media(&unused.name).unwrap();
    assert_eq!(c.media_bytes(&unused.name).unwrap(), WAV);
    assert_eq!(c.check_media().unwrap().unused.len(), 1);
}

#[test]
fn adding_a_deleted_file_again_brings_it_back() {
    let c = collection();
    let first = c.add_media("song.wav", WAV).unwrap();
    c.delete_media(&first.name).unwrap();
    assert!(c.media_file(&first.name).unwrap().unwrap().deleted);
    let again = c.add_media("song.wav", WAV).unwrap();
    assert!(again.new);
    assert!(!again.file.deleted);
    assert_eq!(c.media_files().unwrap().len(), 1);
    assert_eq!(c.media_bytes(&again.name).unwrap(), WAV);
}

#[test]
fn deleting_a_file_that_is_used_is_refused_and_a_missing_one_says_so() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    note(&c, &img(&cat.name), "back");
    let error = c.delete_media(&cat.name).unwrap_err();
    assert_eq!(error, MediaError::StillUsed(cat.name.clone()));
    assert!(error.to_string().contains("still used"));
    assert!(!c.media_file(&cat.name).unwrap().unwrap().deleted);
    let error = c.delete_media("nothing.png").unwrap_err();
    assert!(error.to_string().contains("nothing.png"));
    assert_eq!(
        c.restore_media("nothing.png"),
        Err(MediaError::NotFound("nothing.png".into()))
    );
}

#[test]
fn a_deleted_file_that_a_note_names_stays_alive() {
    // Another device deleted the file as unused while this one added a note that names it. Here the
    // two happen in the other order, which a merge makes the same.
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    c.delete_media(&cat.name).unwrap();
    assert!(c.media_file(&cat.name).unwrap().unwrap().deleted);
    let n = note(&c, &img(&cat.name), "back");
    let file = c.media_file(&cat.name).unwrap().unwrap();
    assert!(!file.deleted, "a named file is alive");
    assert_eq!(c.media_bytes(&cat.name).unwrap(), PNG);
    assert!(!c.media_files().unwrap()[0].deleted);
    let report = c.check_media().unwrap();
    assert!(report.unused.is_empty());
    assert!(report.missing.is_empty());
    // Nothing was written to make it so: the register still says deleted.
    let raw: i64 = c
        .conn
        .query_row("SELECT deleted FROM media_file", [], |r| r.get(0))
        .unwrap();
    assert_eq!(raw, 1);
    // A note in the trash still names it, because restoring the note brings the name back.
    c.delete_note(n).unwrap();
    assert!(!c.media_file(&cat.name).unwrap().unwrap().deleted);
}

// --- bytes that are not here ----------------------------------------------------------------

#[test]
fn a_file_whose_bytes_have_not_arrived_is_known_but_not_served() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    note(&c, &img(&cat.name), "back");
    c.conn.execute("DELETE FROM media_blob", []).unwrap();
    let file = c.media_file(&cat.name).unwrap().unwrap();
    assert!(!file.has_bytes);
    let error = c.media_bytes(&cat.name).unwrap_err();
    assert_eq!(error, MediaError::NoBytes(cat.name.clone()));
    assert!(error.to_string().contains("Sync"));
    let report = c.check_media().unwrap();
    assert_eq!(report.without_bytes.len(), 1);
    assert!(report.missing.is_empty());
    // Adding the file again fills the bytes in.
    let again = c.add_media("cat.png", PNG).unwrap();
    assert!(again.new);
    assert_eq!(c.media_bytes(&cat.name).unwrap(), PNG);
    assert!(c.check_media().unwrap().without_bytes.is_empty());
}

#[test]
fn damaged_bytes_are_never_served() {
    let c = collection();
    let cat = c.add_media("cat.png", PNG).unwrap();
    c.conn
        .execute("UPDATE media_blob SET data = x'00'", [])
        .unwrap();
    assert_eq!(c.media_bytes(&cat.name), Err(MediaError::NoBytes(cat.name)));
}

// --- sync ---------------------------------------------------------------------------------

#[test]
fn two_collections_that_add_the_same_bytes_make_the_same_entity() {
    let (a, b) = two_collections();
    let on_a = a.add_media("cat.png", PNG).unwrap();
    let on_b = b.add_media("kitten.jpeg", PNG).unwrap();
    assert_eq!(on_a.file.id, on_b.file.id);
    assert_eq!(on_a.file.hash, on_b.file.hash);
    assert_eq!(on_a.file.size, on_b.file.size);
    // A name from one collection belongs to the same file in the other.
    assert_eq!(b.media_bytes(&on_a.name).unwrap(), PNG);
    assert_eq!(a.media_bytes(&on_b.name).unwrap(), PNG);
}

#[test]
fn every_register_of_a_media_file_has_a_clock() {
    let c = collection();
    let added = c.add_media("cat.png", PNG).unwrap();
    for register in MEDIA_FILE.registers {
        assert!(
            c.register_clock("media_file", added.file.id, register)
                .unwrap()
                .is_some(),
            "{register}"
        );
    }
}

#[test]
fn the_schema_check_passes_with_media_and_the_guard_refuses_a_raw_write() {
    let c = collection();
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
    assert!(
        c.conn
            .execute(
                "INSERT INTO media_file (id, hash, size) VALUES (x'00', 'ab', 1)",
                [],
            )
            .is_err()
    );
    let added = c.add_media("cat.png", PNG).unwrap();
    assert!(
        c.conn
            .execute(
                "UPDATE media_file SET deleted = 1 WHERE id = ?1",
                [added.file.id]
            )
            .is_err()
    );
    assert!(
        c.conn
            .execute("DELETE FROM media_file WHERE id = ?1", [added.file.id])
            .is_err()
    );
    assert_eq!(c.media_files().unwrap().len(), 1);
}

#[test]
fn a_register_from_a_newer_app_is_kept_and_does_not_break_reading() {
    let c = collection();
    let added = c.add_media("cat.png", PNG).unwrap();
    c.store_unknown_register(&crate::sync::UnknownRegister {
        entity_type: "media_file".to_owned(),
        entity_id: added.file.id,
        field: "future_register".to_owned(),
        value: rusqlite::types::Value::Text("later".into()),
        hlc: crate::sync::Hlc::from_parts(START + 5, 0),
        device: Id::from_bytes(*b"remote-device-01"),
    })
    .unwrap();
    assert_eq!(c.media_file(&added.name).unwrap().unwrap(), added.file);
    assert_eq!(
        c.unknown_registers("media_file", added.file.id)
            .unwrap()
            .len(),
        1
    );
}

// --- a version 9 collection ---------------------------------------------------------------------

#[test]
fn a_version_9_collection_upgrades_and_keeps_its_notes() {
    use crate::collection::Schema;
    let location = std::env::temp_dir().join(format!("fc-media-v9-{}.db", std::process::id()));
    let location = location.to_str().unwrap().to_owned();
    let _ = std::fs::remove_file(&location);
    let clock = Arc::new(ManualClock::new(START));
    let v9 = Schema {
        migrations: &MIGRATIONS[..9],
        tables: &SYNCED_TABLES[..SYNCED_TABLES.len() - 1],
    };
    let old = Collection::create_with(&location, v9, host(&clock, b"installation-one")).unwrap();
    let n = note(&old, "kot", "cat");
    assert_eq!(old.info().unwrap().schema_version, 9);
    old.close().unwrap();

    let upgraded = Collection::open(&location, host(&clock, b"installation-one")).unwrap();
    assert_eq!(upgraded.info().unwrap().schema_version, 10);
    check_schema(&upgraded.conn, SYNCED_TABLES).unwrap();
    assert!(upgraded.note(n).unwrap().is_some());
    assert!(upgraded.media_files().unwrap().is_empty());
    let added = upgraded.add_media("cat.png", PNG).unwrap();
    assert_eq!(upgraded.media_bytes(&added.name).unwrap(), PNG);
    upgraded.close().unwrap();
    let _ = std::fs::remove_file(&location);
    let _ = std::fs::remove_file(format!("{location}-journal"));
}

#[test]
fn media_survives_closing_and_opening() {
    let location = std::env::temp_dir().join(format!("fc-media-reopen-{}.db", std::process::id()));
    let location = location.to_str().unwrap().to_owned();
    let _ = std::fs::remove_file(&location);
    let clock = Arc::new(ManualClock::new(START));
    let c = Collection::create(&location, host(&clock, b"installation-one")).unwrap();
    let added = c.add_media("cat.png", PNG).unwrap();
    c.close().unwrap();
    let c = Collection::open(&location, host(&clock, b"installation-one")).unwrap();
    assert_eq!(c.media_bytes(&added.name).unwrap(), PNG);
    c.close().unwrap();
    let _ = std::fs::remove_file(&location);
}

// --- rough input --------------------------------------------------------------------------

#[test]
fn random_names_and_bytes_never_break_adding_or_reading() {
    let c = collection();
    // A deterministic generator, so a failure repeats.
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let pieces = [
        "a", "Z", "-", "_", ".", "..", "/", "\\", " ", "\"", "'", "<", ">", "&", "]", "[sound:",
        "ż", "日", "\n", "\0", "%", "#", "?", ":", "é", "{{", "}}", "url(", "png",
    ];
    for round in 0..3_000 {
        let mut original = String::new();
        for _ in 0..(next() % 8) {
            original.push_str(pieces[(next() % pieces.len() as u64) as usize]);
        }
        let bytes = format!("file {} {}", round % 40, next() % 3).into_bytes();
        let added = c.add_media(&original, &bytes).unwrap();
        assert_eq!(c.media_bytes(&added.name).unwrap(), bytes, "{original:?}");
        assert_eq!(
            digits_in(&added.name).unwrap(),
            &added.file.hash[..HASH_DIGITS]
        );
        // Reading any text as a name never fails.
        let _ = c.media_file(&original).unwrap();
        let _ = c.media_bytes(&original);
    }
    // Only distinct contents were stored.
    assert!(c.media_files().unwrap().len() <= 120);
    check_schema(&c.conn, SYNCED_TABLES).unwrap();
}
