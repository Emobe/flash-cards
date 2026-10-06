//! `fc merge`: giving the changes of one collection file to another, to try a merge by hand
//! before any sync exists (ADR 0008, part 12).

use fc_core::sync::Selection;

use super::{Failure, open, plural};

/// Applies everything in `from` to `into`. `from` is only read.
pub(super) fn merge(from: &str, into: &str) -> Result<String, Failure> {
    if from == into {
        return Err(Failure::Usage(
            "Give two different files to merge.".to_owned(),
        ));
    }
    let source = open(from)?;
    let changes = source.changes(Selection::All)?;
    source.close()?;
    let target = open(into)?;
    let report = target.merge(&changes)?;
    target.close()?;
    let mut text = format!(
        "Merged {from} into {into}\n\
         Registers applied: {}\n\
         Registers ignored (older or equal): {}\n\
         Unknown registers kept: {}\n\
         Rows added: {}\n\
         Unknown rows kept: {}\n\
         Notes reconciled: {}\n\
         Cards rebuilt: {}",
        report.registers_applied,
        report.registers_ignored,
        report.unknown_registers,
        report.rows_added,
        report.unknown_rows,
        report.notes_reconciled,
        report.cards_rebuilt
    );
    if !report.rejected.is_empty() {
        text.push_str(&format!(
            "\n{} could not be applied:",
            plural(report.rejected.len(), "change")
        ));
        for rejected in &report.rejected {
            text.push_str(&format!(
                "\n  {} {}{}: {}",
                rejected.entity,
                rejected.id,
                rejected
                    .field
                    .as_ref()
                    .map_or(String::new(), |f| format!(" {f}")),
                rejected.reason
            ));
        }
    }
    Ok(text)
}
