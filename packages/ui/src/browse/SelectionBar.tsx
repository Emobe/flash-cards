import { CoreError, type SearchMode, type TargetSummary, type UndoToken } from "core-client";
import { useState } from "react";
import { CardInfoWindow } from "../card/CardInfoWindow";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { TagInput, withTyped } from "../screens/add/TagInput";
import { CloseIcon } from "../shell/icons";
import { ChangeTypeWindow } from "./ChangeTypeWindow";
import type { Names } from "./chips";
import { DueSheet } from "./DueSheet";
import { PickerDialog } from "./PickerDialog";
import { targetOf, type useSelection } from "./useSelection";

type Selection = ReturnType<typeof useSelection>;
type Sheet = "move" | "add" | "remove" | "delete" | "due" | "reset" | "change" | "more" | "info";

/** `1 card`, `18 cards`. */
function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

const cardsText = (n: number) => plural(n, "card", "cards");
const notesText = (n: number) => plural(n, "note", "notes");

/**
 * What the selection is, in the bar and above Move: "12 notes (18 cards)" in Notes view, since a
 * card action reaches every card of the notes (ADR 0014 decision 3), and "5 cards" in Cards view.
 */
export function selectionText(
  size: number,
  mode: SearchMode,
  summary: TargetSummary | null,
): string {
  if (size === 0) return "Nothing selected";
  if (mode === "cards") return cardsText(size);
  return summary && summary.cards !== size
    ? `${notesText(size)} (${cardsText(summary.cards)})`
    : notesText(size);
}

/** The notes a note action (tags, delete) reaches. */
function notesReached(size: number, mode: SearchMode, summary: TargetSummary | null): string {
  if (mode === "notes") return notesText(size);
  return summary ? notesText(summary.notes) : `the notes of ${cardsText(size)}`;
}

/** The cards a card action reaches: "On 18 cards". */
function cardsReached(size: number, mode: SearchMode, summary: TargetSummary | null): string {
  if (summary) return `On ${cardsText(summary.cards)}`;
  return mode === "cards" ? `On ${cardsText(size)}` : `On the cards of ${notesText(size)}`;
}

/** "Reset 15 cards to new?", leaving out the cards that are new already. */
function resetQuestion(size: number, mode: SearchMode, summary: TargetSummary | null): string {
  if (!summary)
    return mode === "cards" ? `Reset ${cardsText(size)} to new?` : "Reset the cards to new?";
  return `Reset ${cardsText(Math.max(0, summary.cards - summary.newCards))} to new?`;
}

function deleteQuestion(size: number, mode: SearchMode, summary: TargetSummary | null): string {
  if (!summary) {
    return mode === "notes"
      ? `Delete ${notesText(size)} and their cards?`
      : `Delete the notes of ${cardsText(size)} and all their cards?`;
  }
  const their = summary.notes === 1 ? "its" : "their";
  return `Delete ${notesText(summary.notes)} and ${their} ${cardsText(summary.noteCards)}?`;
}

/**
 * The selection bar (ADR 0014 decisions 2 and 3): leave, the count, Select all, and the actions.
 * Under 600 px it sits in `BottomAction` in place of the search panel, with Move, Tag, Suspend,
 * Delete and More (Set due date, Reset to new, Change note type, Remove tags); from 600 px it sits
 * above the list with Move, Add tags, Remove tags, Suspend, Delete and More (Set due date, Reset to
 * new, Change note type), since every action does not fit in one row. Move, tags and suspend act at once (after their sheet); Delete, Set due
 * date and Reset ask first. `onDone` gets each change's undo
 * token, and whether the selected rows are gone (a delete, or a type change in Cards view, which
 * replaces the cards): the screen then clears the selection.
 */
export function SelectionBar({
  selection,
  mode,
  onSelectAll,
  total,
  wide,
  names,
  summary,
  infoCard,
  onDone,
}: {
  selection: Selection;
  mode: SearchMode;
  /** Chooses every row the search matches, not only the loaded ones. */
  onSelectAll: () => Promise<void>;
  /** How many rows the search matched. */
  total: number;
  wide: boolean;
  names: Names | null;
  summary: TargetSummary | null;
  /** The card to show Card info for: set when exactly one row is selected, otherwise null. */
  infoCard: string | null;
  onDone: (undo: UndoToken | null, deleted: boolean) => void;
}) {
  const core = useCore();
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const { size, ids } = selection;
  const target = targetOf(ids, mode);
  const allSuspended = summary !== null && summary.cards > 0 && summary.suspended === summary.cards;

  async function run(
    action: () => Promise<{ undo: UndoToken | null }>,
    deleted = false,
  ): Promise<void> {
    setBusy(true);
    setProblem(null);
    try {
      const out = await action();
      setSheet(null);
      onDone(out.undo, deleted);
    } catch (failure) {
      setSheet(null);
      setProblem(failure instanceof CoreError ? failure.message : "That did not work. Try again.");
    } finally {
      setBusy(false);
    }
  }

  async function selectAll() {
    setBusy(true);
    setProblem(null);
    try {
      await onSelectAll();
    } catch (failure) {
      setProblem(
        failure instanceof CoreError ? failure.message : "Could not select them all. Try again.",
      );
    } finally {
      setBusy(false);
    }
  }

  const off = busy || size === 0;
  const suspend = (
    <button
      type="button"
      className="button"
      disabled={off}
      onClick={() =>
        void run(() => core.call(allSuspended ? "unsuspendCards" : "suspendCards", { target }))
      }
    >
      {allSuspended ? "Unsuspend" : "Suspend"}
    </button>
  );
  const action = (label: string, open: Sheet, danger = false) => (
    <button
      type="button"
      className={danger ? "button button-danger-text" : "button"}
      disabled={off}
      onClick={() => {
        setProblem(null);
        setSheet(open);
      }}
    >
      {label}
    </button>
  );

  return (
    <div className="selection-bar" role="toolbar" aria-label="Selection" data-wide={wide}>
      <div className="selection-head">
        <button
          type="button"
          className="selection-leave"
          aria-label="Leave selection"
          title="Leave selection (Escape)"
          onClick={selection.leave}
        >
          <CloseIcon />
        </button>
        <span className="selection-count" role="status">
          {selectionText(size, mode, summary)}
        </span>
        {total > size && (
          <button type="button" className="button" disabled={busy} onClick={() => void selectAll()}>
            Select all {total.toLocaleString()}
          </button>
        )}
      </div>
      <div className="selection-actions">
        {action("Move", "move")}
        {wide ? (
          <>
            {action("Add tags", "add")}
            {action("Remove tags", "remove")}
          </>
        ) : (
          action("Tag", "add")
        )}
        {suspend}
        {action("Delete", "delete", true)}
        {action("More", "more")}
      </div>
      {problem && (
        <p className="browse-problem" role="alert">
          {problem}
        </p>
      )}

      {sheet === "move" && (
        <PickerDialog
          title="Move to a deck"
          findLabel="Find a deck"
          applyLabel="Move"
          clearable={false}
          busy={busy}
          multiple={false}
          note={selectionText(size, mode, summary)}
          options={(names?.decks ?? []).map((d) => ({
            value: d.id,
            label: d.name,
            depth: d.depth,
            match: d.path,
          }))}
          selected={[]}
          filterable
          onApply={([deckId]) => {
            if (deckId) void run(() => core.call("moveCards", { target, deckId }));
          }}
          onClose={() => setSheet(null)}
        />
      )}
      {(sheet === "add" || sheet === "remove") && (
        <TagSheet
          initial={sheet}
          known={names?.tags ?? []}
          note={`On ${notesReached(size, mode, summary)}`}
          busy={busy}
          onApply={(tagMode, tags) =>
            void run(() =>
              core.call(tagMode === "add" ? "addTags" : "removeTags", { target, tags }),
            )
          }
          onClose={() => setSheet(null)}
        />
      )}
      {sheet === "delete" && (
        <Dialog title={deleteQuestion(size, mode, summary)} onClose={() => setSheet(null)}>
          <p>You can undo it right after.</p>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setSheet(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button-danger"
              disabled={busy}
              onClick={() => void run(() => core.call("deleteNotes", { target }), true)}
            >
              Delete
            </button>
          </div>
        </Dialog>
      )}
      {sheet === "due" && (
        <DueSheet
          note={cardsReached(size, mode, summary)}
          newCards={summary?.newCards ?? 0}
          busy={busy}
          onApply={(fromDays, toDays) =>
            void run(() => core.call("setDueDate", { target, fromDays, toDays }))
          }
          onClose={() => setSheet(null)}
        />
      )}
      {sheet === "reset" && (
        <Dialog title={resetQuestion(size, mode, summary)} onClose={() => setSheet(null)}>
          <p>
            They start again as new cards. Their review history is kept. You can undo it right
            after.
            {summary && summary.newCards > 0 && (
              <>
                {" "}
                {summary.newCards === 1
                  ? "1 card is new already and stays as it is."
                  : `${summary.newCards.toLocaleString()} cards are new already and stay as they are.`}
              </>
            )}
          </p>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setSheet(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button-danger"
              disabled={busy}
              onClick={() => void run(() => core.call("resetCards", { target }))}
            >
              Reset
            </button>
          </div>
        </Dialog>
      )}
      {sheet === "change" && summary && (
        <ChangeTypeWindow
          target={target}
          notes={summary.notes}
          noteTypeId={summary.noteTypeId}
          onDone={(out) => {
            setSheet(null);
            onDone(out.undo, mode === "cards");
          }}
          onClose={() => setSheet(null)}
        />
      )}
      {sheet === "info" && infoCard !== null && (
        <CardInfoWindow cardId={infoCard} onClose={() => setSheet(null)} />
      )}
      {sheet === "more" && (
        <Dialog title="More actions" variant="sheet" onClose={() => setSheet(null)}>
          <div className="selection-more">
            <button
              type="button"
              className="button"
              disabled={infoCard === null}
              onClick={() => setSheet("info")}
            >
              Card info
            </button>
            <button type="button" className="button" onClick={() => setSheet("due")}>
              Set due date
            </button>
            <button type="button" className="button" onClick={() => setSheet("reset")}>
              Reset to new
            </button>
            <button
              type="button"
              className="button"
              disabled={summary === null}
              onClick={() => setSheet("change")}
            >
              Change note type
            </button>
            {!wide && (
              <button type="button" className="button" onClick={() => setSheet("remove")}>
                Remove tags
              </button>
            )}
          </div>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setSheet(null)}>
              Cancel
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}

/** Add or remove tags on the selection's notes: a switch and the tag box (`TagInput`). */
function TagSheet({
  initial,
  known,
  note,
  busy,
  onApply,
  onClose,
}: {
  initial: "add" | "remove";
  known: string[];
  note: string;
  busy: boolean;
  onApply: (mode: "add" | "remove", tags: string[]) => void;
  onClose: () => void;
}) {
  const [tagMode, setTagMode] = useState(initial);
  const [tags, setTags] = useState<string[]>([]);
  const [typed, setTyped] = useState("");
  const all = withTyped(tags, typed);
  return (
    <Dialog title="Tags" variant="sheet" onClose={onClose}>
      <form
        className="tag-sheet"
        onSubmit={(event) => {
          event.preventDefault();
          if (all.length > 0) onApply(tagMode, all);
        }}
      >
        <fieldset className="segmented">
          <legend className="visually-hidden">Add or remove</legend>
          {(["add", "remove"] as const).map((m) => (
            <button
              key={m}
              type="button"
              className="button"
              aria-pressed={tagMode === m}
              onClick={() => setTagMode(m)}
            >
              {m === "add" ? "Add" : "Remove"}
            </button>
          ))}
        </fieldset>
        <p className="picker-note">{note}</p>
        <TagInput tags={tags} typed={typed} known={known} onTags={setTags} onTyped={setTyped} />
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="button button-primary"
            disabled={busy || all.length === 0}
          >
            {tagMode === "add" ? "Add tags" : "Remove tags"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
