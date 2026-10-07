//! `fc bench`: times the operations that ADRs 0006 to 0008 said to measure on a large collection
//! (step 1.14a). It works on a copy of the file, so the collection is never changed.
//!
//! Reads are run three times and the fastest is shown (the first run has a cold page cache, and the
//! fastest is the one least disturbed by other programs). Writes run once, in the order listed,
//! each on the copy the one before left.

use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fc_core::backup::ExportOptions;
use fc_core::collection::Collection;
use fc_core::notetype::builtin;
use fc_core::scheduling::Rating;
use fc_core::search::{SearchOptions, Sort, SortKey};
use fc_core::stats::DayRange;
use fc_core::study::Next;
use fc_core::sync::Selection;

use super::{Failure, host, plural};

/// The copy and the files next to it, removed when the bench ends or fails.
struct Scratch(Vec<PathBuf>);

impl Scratch {
    fn path(&mut self, file: &str, tag: &str) -> String {
        let path = format!("{file}.bench-{}-{tag}", std::process::id());
        for suffix in ["", "-wal", "-shm", ".installation"] {
            self.0.push(PathBuf::from(format!("{path}{suffix}")));
        }
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Row {
    name: String,
    time: Option<Duration>,
    detail: String,
}

#[derive(Default)]
struct Report(Vec<Row>);

impl Report {
    /// Runs `f` `runs` times and keeps the fastest. An error is shown, not raised: one query that
    /// the collection cannot answer should not hide the other timings.
    fn measure<E: ToString>(
        &mut self,
        name: &str,
        runs: u32,
        mut f: impl FnMut() -> Result<String, E>,
    ) {
        let mut best: Option<Duration> = None;
        let mut detail = String::new();
        for _ in 0..runs {
            let started = Instant::now();
            match f() {
                Ok(text) => {
                    let took = started.elapsed();
                    best = Some(best.map_or(took, |b| b.min(took)));
                    detail = text;
                }
                Err(error) => {
                    detail = format!("failed: {}", error.to_string());
                    best = None;
                    break;
                }
            }
        }
        self.0.push(Row {
            name: name.to_owned(),
            time: best,
            detail,
        });
    }

    fn text(&self) -> String {
        let width = self.0.iter().map(|r| r.name.len()).max().unwrap_or(0);
        self.0
            .iter()
            .map(|row| {
                let time = row.time.map_or("       -".to_owned(), |t| {
                    format!("{:>8.1}", t.as_secs_f64() * 1000.0)
                });
                format!("{:<width$}  {time} ms  {}", row.name, row.detail)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The first word of a note's first field, with the HTML and cloze markers taken out.
fn first_word(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .find(|w| w.chars().count() >= 3 && !w.starts_with('c'))
        .map(str::to_owned)
}

pub fn bench(file: &str) -> Result<String, Failure> {
    if !std::path::Path::new(file).exists() {
        return Err(Failure::Core(format!("There is no collection at {file}.")));
    }
    let mut scratch = Scratch(Vec::new());
    let copy = scratch.path(file, "copy");
    std::fs::copy(file, &copy)
        .map_err(|e| Failure::Core(format!("Could not copy {file} to {copy}: {e}")))?;
    let size = std::fs::metadata(&copy).map(|m| m.len()).unwrap_or(0);
    let mut report = Report::default();

    let started = Instant::now();
    let collection = Collection::open(&copy, host::host_for(&copy, true).map_err(Failure::Core)?)?;
    report.0.push(Row {
        name: "open (with a new device ID)".to_owned(),
        time: Some(started.elapsed()),
        detail: String::new(),
    });

    // What there is to work with.
    let types = collection.note_types()?;
    let mut notes = 0;
    let mut sample_words: Vec<String> = Vec::new();
    let mut sample_first_field = None;
    for note_type in &types {
        let of_type = collection.notes(note_type.id)?;
        notes += of_type.len();
        if note_type.id == builtin::basic() {
            sample_first_field = of_type
                .first()
                .and_then(|n| n.fields.first().map(|f| f.value.clone()));
        }
        for note in of_type.iter().take(2) {
            if let Some(word) = note.fields.first().and_then(|f| first_word(&f.value)) {
                sample_words.push(word);
            }
        }
    }
    let decks = collection.decks()?;
    let tags = collection.tags()?;
    let top_deck = decks.first().map(|d| d.path.clone());
    let study_deck = decks
        .first()
        .map_or_else(fc_core::deck::default_deck, |d| d.id);
    let top_tag = tags.first().map(|t| t.name.clone());
    let mut header = format!(
        "{file}: {:.1} MB, {}, {}, {}",
        size as f64 / 1_048_576.0,
        plural(notes, "note"),
        plural(decks.len(), "deck"),
        plural(tags.len(), "tag"),
    );
    let events = collection
        .daily_counts(DayRange::last(collection.study_today()?, 3650), None)?
        .iter()
        .map(|d| u64::from(d.answers()))
        .sum::<u64>();
    header.push_str(&format!(
        ", {} on record",
        plural(events as usize, "answer")
    ));

    // Reads.
    report.measure("deck counts", 3, || {
        let counts = collection.deck_counts()?;
        Ok::<_, fc_core::study::StudyError>(plural(counts.len(), "deck"))
    });
    report.measure("next card (first deck and its sub-decks)", 3, || {
        let next = collection.next_card(study_deck)?;
        Ok::<_, fc_core::study::StudyError>(
            match next {
                Next::Card { counts, .. } => counts,
                Next::Waiting { counts, .. } | Next::Done { counts } => counts,
            }
            .review
            .to_string()
                + " review cards",
        )
    });
    let mut queries: Vec<(String, String)> = [
        ("all cards, 50 by sort field", ""),
        ("is:due", "is:due"),
        ("is:new", "is:new"),
        ("suspended or buried", "is:suspended or is:buried"),
        ("rated:30:1", "rated:30:1"),
        ("difficulty:>7", "difficulty:>7"),
    ]
    .iter()
    .map(|(name, query)| ((*name).to_owned(), (*query).to_owned()))
    .collect();
    if let Some(deck) = &top_deck {
        queries.push((format!("deck:{deck}"), format!("deck:\"{deck}\"")));
    }
    if let Some(tag) = &top_tag {
        queries.push((format!("tag:{tag}"), format!("tag:\"{tag}\"")));
    }
    if let Some(word) = sample_words.first() {
        queries.push((format!("one word ({word})"), word.clone()));
    }
    if let [first, .., last] = sample_words.as_slice() {
        queries.push((
            format!("two words ({first} {last})"),
            format!("{first} {last}"),
        ));
    }
    for (name, query) in queries {
        let sort = if query.is_empty() {
            Sort {
                key: SortKey::SortField,
                descending: false,
            }
        } else {
            Sort::default()
        };
        report.measure(&format!("search {name}"), 3, || {
            let page = collection.search(
                &query,
                &SearchOptions {
                    sort,
                    ..SearchOptions::default()
                },
            )?;
            Ok::<_, fc_core::search::SearchError>(format!("{} found", page.total))
        });
    }
    report.measure("tags()", 3, || {
        Ok::<_, fc_core::tag::TagError>(plural(collection.tags()?.len(), "tag"))
    });
    if let Some(tag) = &top_tag {
        report.measure(&format!("notes_with_tag {tag} (and inside)"), 3, || {
            Ok::<_, fc_core::tag::TagError>(plural(
                collection.notes_with_tag(tag, true)?.len(),
                "note",
            ))
        });
    }
    if let Some(value) = &sample_first_field {
        report.measure("find_duplicates (Basic)", 3, || {
            Ok::<_, fc_core::note::NoteError>(plural(
                collection.find_duplicates(builtin::basic(), value)?.len(),
                "duplicate",
            ))
        });
    }
    report.measure("daily counts, 365 days", 3, || {
        let today = collection.study_today()?;
        let days = collection.daily_counts(DayRange::last(today, 365), None)?;
        Ok::<_, fc_core::study::StudyError>(plural(days.len(), "day"))
    });
    report.measure("due forecast, 30 days", 3, || {
        collection.due_forecast(30, None)?;
        Ok::<_, fc_core::study::StudyError>(String::new())
    });
    report.measure("export, with history (in memory)", 1, || {
        let mut out = Cursor::new(Vec::new());
        collection.export_backup(&mut out, &ExportOptions::default())?;
        Ok::<_, fc_core::backup::BackupError>(format!(
            "{:.1} MB",
            out.get_ref().len() as f64 / 1_048_576.0
        ))
    });
    let mut changes = None;
    report.measure("changes(All)", 1, || {
        let found = collection.changes(Selection::All)?;
        let text = format!(
            "{}, {}",
            plural(found.registers.len(), "register"),
            plural(found.rows.len(), "row")
        );
        changes = Some(found);
        Ok::<_, fc_core::collection::CollectionError>(text)
    });
    if let Some(changes) = changes {
        let target = scratch.path(file, "merge");
        let empty = Collection::create(
            &target,
            host::host_for(&target, true).map_err(Failure::Core)?,
        )?;
        report.measure("merge into a new collection", 1, || {
            let merged = empty.merge(&changes)?;
            Ok::<_, fc_core::collection::CollectionError>(format!(
                "{} applied, {} rejected",
                plural(merged.registers_applied, "register"),
                merged.rejected.len()
            ))
        });
        empty.close()?;
    }
    report.measure("rebuild_schedule (every card)", 1, || {
        collection.rebuild_schedule()?;
        Ok::<_, fc_core::study::StudyError>(String::new())
    });

    // Writes, each on the copy the one before left.
    let (first, second) = {
        let basic = collection.note_type(builtin::basic())?;
        let basic =
            basic.ok_or_else(|| Failure::Core("The Basic note type is gone.".to_owned()))?;
        (basic.fields[0].id, basic.fields[1].id)
    };
    let mut added = None;
    report.measure("add a Basic note", 1, || {
        let note = collection.add_note(
            builtin::basic(),
            &[(first, "bench front"), (second, "bench back")],
        )?;
        added = Some(note.id);
        Ok::<_, fc_core::note::NoteError>(String::new())
    });
    if let Some(note) = added {
        report.measure("add a tag to the note", 1, || {
            collection.add_tags(&[note], &["bench::tag"])?;
            Ok::<_, fc_core::tag::TagError>(String::new())
        });
    }
    report.measure("answer the next card", 1, || {
        match collection.next_card(study_deck)? {
            Next::Card { card, .. } => {
                collection.answer(card, Rating::Good, 3_000)?;
                Ok::<_, fc_core::study::StudyError>(String::new())
            }
            _ => Ok(" nothing to answer".to_owned()),
        }
    });
    if let Some(tag) = tags
        .iter()
        .find(|t| !t.name.contains(fc_core::tag::SEPARATOR))
        .map(|t| t.name.clone())
    {
        report.measure(&format!("rename tag {tag} (and inside)"), 1, || {
            let changed = collection.rename_tag(&tag, &format!("{tag}-renamed"))?;
            Ok::<_, fc_core::tag::TagError>(plural(changed, "note"))
        });
    }
    let basic_notes = collection.notes(builtin::basic())?.len();
    report.measure(
        &format!("add a template to Basic ({})", plural(basic_notes, "note")),
        1,
        || {
            collection.add_template(builtin::basic(), "Bench", "{{Back}}", "{{Front}}")?;
            Ok::<_, fc_core::notetype::NoteTypeError>(String::new())
        },
    );
    collection.close()?;
    Ok(format!(
        "{header}\nTimes are for this machine; the fastest of three runs for reads, one run for writes.\n\n{}",
        report.text()
    ))
}
