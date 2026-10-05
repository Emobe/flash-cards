//! Rendering a stored card (step 1.4). The template language is in `crate::template`.

use rusqlite::OptionalExtension;

use super::generate::{reader, values_by_name};
use crate::collection::{Collection, CollectionError};
use crate::id::Id;
use crate::template::{self, RenderError, RenderedCard};

impl Collection {
    /// The front and back of a live card as HTML for the card frame, and the media files they name.
    /// Fails with `RenderError::Template` if the card's template has a mistake, saying where, and
    /// with `NotFound` if the card, its note or its template is deleted.
    pub fn render_card(&self, card: Id) -> Result<RenderedCard, RenderError> {
        let row: Option<(Id, Id, Id, u32)> = self
            .conn
            .query_row(
                "SELECT c.note, n.note_type, c.template, c.ordinal FROM card c
                 JOIN note n ON n.id = c.note
                 WHERE c.id = ?1 AND c.deleted = 0 AND n.deleted = 0",
                [card],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((note, note_type, template_id, ordinal)) = row else {
            return Err(RenderError::NotFound);
        };
        let note_type = self
            .note_type(note_type)
            .map_err(CollectionError::from)?
            .ok_or(RenderError::NotFound)?;
        let template = note_type
            .templates
            .iter()
            .find(|t| t.id == template_id)
            .ok_or(RenderError::NotFound)?;
        let state = self.note_state(note)?;
        let by_name = values_by_name(&note_type, &state.values);
        let value = reader(&by_name);
        template::render(&note_type, template, &value, ordinal).map_err(RenderError::Template)
    }
}
