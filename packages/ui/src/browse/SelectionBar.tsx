import { CoreError, type SearchMode, type TargetSummary, type UndoToken } from "core-client";
import { useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { TagInput, withTyped } from "../screens/add/TagInput";
import { CloseIcon } from "../shell/icons";
import type { Names } from "./chips";
import { PickerDialog } from "./PickerDialog";
import { targetOf, type useSelection } from "./useSelection";

type Selection = ReturnType<typeof useSelection>;
type Sheet = "move" | "add" | "remove" | "delete" | "more";

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
 * Delete and More; from 600 px it sits above the list with every action in one row. Move, tags and
 * suspend act at once (after their sheet); Delete asks first. `onDone` gets each change's undo
 * token, and whether it was a delete (the screen then clears the selection).
 */
export function SelectionBar({
  selection,
  mode,
  onSelectAll,
  total,
  wide,
  names,
  summary,
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
        {!wide && action("More", "more")}
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
      {sheet === "more" && (
        <Dialog title="More actions" variant="sheet" onClose={() => setSheet(null)}>
          <div className="selection-more">
            <button type="button" className="button" onClick={() => setSheet("remove")}>
              Remove tags
            </button>
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
