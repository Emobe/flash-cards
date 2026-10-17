import {
  CoreError,
  type DeckSummary,
  type NoteDetails,
  type NoteTypeSummary,
  type UndoToken,
} from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { TagInput, withTyped } from "../screens/add/TagInput";
import { WarningIcon } from "../shell/icons";
import { BottomAction } from "../shell/slot";
import { highestCloze } from "./commands";
import { FieldEditor } from "./FieldEditor";
import { loadField, roundTrips, saveField } from "./html";
import { Toolbar } from "./Toolbar";
import { useFieldEditing } from "./useFieldEditing";
import "../screens/add/add.css";
import "./note-editor.css";

const DUPLICATE_DELAY_MS = 400;

/** What the editor did, for the screen under it to say so and offer Undo. */
export type EditorResult =
  | { kind: "saved"; undo: UndoToken | null; cardsRemoved: string[] }
  | { kind: "deleted"; undo: UndoToken | null };

type Loaded =
  | { status: "loading" }
  | { status: "failed"; error: CoreError }
  | {
      status: "ready";
      note: NoteDetails;
      noteType: NoteTypeSummary;
      decks: DeckSummary[];
      knownTags: string[];
      /** Fields shown as HTML text because the editor would change them (ADR 0011). */
      htmlBox: Set<string>;
      /** Each field as the editor would save it untouched, so loading alone is not a change. */
      baseline: Record<string, string>;
    };

function asCoreError(error: unknown, fallback: string): CoreError {
  return error instanceof CoreError ? error : new CoreError("internal", fallback);
}

function cardsText(count: number): string {
  return `${count} ${count === 1 ? "card" : "cards"}`;
}

function sameTags(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((tag, i) => tag === b[i]);
}

/**
 * Edit one note (step 3.2b, ADR 0014 decision 1), as a layer over the screen it was opened from:
 * the fields, the tags and the deck, saved explicitly in one `updateNote`. Back or Escape with
 * changes asks first. A field the editor cannot show without changing it opens as its HTML in a
 * plain text box, shown as text and never put in the page.
 */
export function NoteEditor({
  noteId,
  onClose,
  onDone,
}: {
  noteId: string;
  /** Closed with nothing written. */
  onClose: () => void;
  /** Saved or deleted. The screen closes the editor and says what happened. */
  onDone: (result: EditorResult) => void;
}) {
  const core = useCore();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [values, setValues] = useState<Record<string, string>>({});
  const [tags, setTags] = useState<string[]>([]);
  const [typedTag, setTypedTag] = useState("");
  /** The deck chosen here, or null while the cards stay where they are. */
  const [deckId, setDeckId] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [duplicate, setDuplicate] = useState(false);
  const [asking, setAsking] = useState<"discard" | "delete" | null>(null);
  const editing = useFieldEditing({ status: setStatus, error: setError });
  const saving = useRef(false);
  const formRef = useRef<HTMLFormElement>(null);

  const load = useCallback(() => {
    setLoaded({ status: "loading" });
    Promise.all([
      core.call("getNote", { noteId }),
      core.call("getNoteTypes", null),
      core.call("getDeckList", null),
      core.call("getTags", null),
    ])
      .then(([note, noteTypes, decks, known]) => {
        const noteType = noteTypes.noteTypes.find((t) => t.id === note.noteTypeId);
        if (!noteType) throw new CoreError("notFound", "That note's type no longer exists.");
        const htmlBox = new Set<string>();
        const baseline: Record<string, string> = {};
        const start: Record<string, string> = {};
        for (const { fieldId, value } of note.fields) {
          const plain = !roundTrips(value);
          if (plain) htmlBox.add(fieldId);
          baseline[fieldId] = plain ? value : saveField(loadField(value));
          start[fieldId] = baseline[fieldId];
        }
        setValues(start);
        setTags(note.tags);
        setTypedTag("");
        setDeckId(null);
        setLoaded({
          status: "ready",
          note,
          noteType,
          decks: decks.decks,
          knownTags: known.tags,
          htmlBox,
          baseline,
        });
      })
      .catch((failure: unknown) =>
        setLoaded({
          status: "failed",
          error: asCoreError(failure, "Something went wrong while opening the note."),
        }),
      );
  }, [core, noteId]);
  useEffect(load, [load]);

  const ready = loaded.status === "ready" ? loaded : null;
  const noteType = ready?.noteType ?? null;
  const isCloze = noteType?.kind === "cloze";
  const startDecks = ready?.note.deckIds ?? [];
  const finalTags = withTyped(tags, typedTag);
  const changedFields = ready
    ? ready.note.fields
        .map((f) => f.fieldId)
        .filter((id) => (values[id] ?? "") !== ready.baseline[id])
    : [];
  const deckChanged = deckId !== null && !(startDecks.length === 1 && startDecks[0] === deckId);
  const dirty =
    ready !== null &&
    (changedFields.length > 0 || !sameTags(finalTags, ready.note.tags) || deckChanged);

  const latestValues = useRef({ noteType, values });
  latestValues.current = { noteType, values };
  const highest = useCallback(() => {
    const { noteType, values } = latestValues.current;
    return highestCloze((noteType?.fields ?? []).map((f) => values[f.id] ?? ""));
  }, []);

  // ---- The duplicate warning, leaving out this note ----

  const firstField = noteType?.fields[0];
  const firstHtml = firstField ? (values[firstField.id] ?? "") : "";
  const noteTypeId = noteType?.id ?? null;
  useEffect(() => {
    if (!noteTypeId || !firstHtml.trim()) {
      setDuplicate(false);
      return;
    }
    let current = true;
    const timer = window.setTimeout(() => {
      core
        .call("findDuplicates", { noteTypeId, value: firstHtml })
        .then((found) => current && setDuplicate(found.noteIds.some((id) => id !== noteId)))
        .catch(() => current && setDuplicate(false));
    }, DUPLICATE_DELAY_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [core, noteId, noteTypeId, firstHtml]);

  // ---- Closing, saving, deleting ----

  function requestClose() {
    if (saving.current) return;
    if (dirty) setAsking("discard");
    else onClose();
  }

  async function save() {
    if (!ready || saving.current || editing.mediaBusy) return;
    if (!dirty) {
      onClose();
      return;
    }
    saving.current = true;
    setPending(true);
    setError(null);
    try {
      const out = await core.call("updateNote", {
        noteId,
        fields: changedFields.map((fieldId) => ({ fieldId, value: values[fieldId] ?? "" })),
        tags: finalTags,
        ...(deckChanged && deckId ? { deckId } : {}),
      });
      onDone({ kind: "saved", undo: out.undo, cardsRemoved: out.cardsRemoved });
    } catch (failure) {
      setError(asCoreError(failure, "The note could not be saved. Try again.").message);
    } finally {
      saving.current = false;
      setPending(false);
    }
  }

  async function remove() {
    if (saving.current) return;
    saving.current = true;
    setPending(true);
    setError(null);
    try {
      const out = await core.call("deleteNotes", { target: { notes: [noteId] } });
      onDone({ kind: "deleted", undo: out.undo });
    } catch (failure) {
      setAsking(null);
      setError(asCoreError(failure, "The note could not be deleted. Try again.").message);
    } finally {
      saving.current = false;
      setPending(false);
    }
  }

  const saveRef = useRef(save);
  saveRef.current = save;
  const loadedStatus = loaded.status;
  // biome-ignore lint/correctness/useExhaustiveDependencies: the form only exists once loaded, so the listener is attached again when the status changes.
  useEffect(() => {
    const form = formRef.current;
    if (!form) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if ((event.ctrlKey || event.metaKey) && (key === "enter" || key === "s")) {
        event.preventDefault();
        void saveRef.current();
      }
    };
    form.addEventListener("keydown", onKeyDown);
    return () => form.removeEventListener("keydown", onKeyDown);
  }, [loadedStatus]);

  return (
    <Dialog title="Edit note" variant="layer" onClose={requestClose}>
      {loaded.status === "loading" && <p role="status">Opening...</p>}
      {loaded.status === "failed" && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{loaded.error.message}</p>
          <button type="button" className="button" onClick={load}>
            Try again
          </button>
        </div>
      )}
      {ready && noteType && (
        // Ctrl+Enter or Ctrl+S anywhere in the form saves. Plain Enter in a text box must not.
        <form
          className="add note-editor"
          aria-label="Edit the note"
          onSubmit={(event) => event.preventDefault()}
          ref={formRef}
        >
          <div className="add-choices">
            <div className="add-choice">
              <span className="add-label">Type</span>
              <span className="note-editor-type">{noteType.name}</span>
            </div>
            <div className="add-choice">
              <label htmlFor="note-editor-deck" className="add-label">
                Deck
              </label>
              <select
                id="note-editor-deck"
                value={deckId ?? (startDecks.length === 1 ? (startDecks[0] ?? "") : "")}
                onChange={(e) => setDeckId(e.target.value)}
              >
                {startDecks.length !== 1 && (
                  <option value="" disabled>
                    {startDecks.length === 0 ? "No cards" : "Several decks"}
                  </option>
                )}
                {ready.decks.map((deck) => (
                  <option key={deck.id} value={deck.id}>
                    {deck.path}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {noteType.fields.map((field, index) => {
            const labelId = `note-editor-field-${field.id}`;
            const plain = ready.htmlBox.has(field.id);
            return (
              <div className="add-field" key={field.id}>
                {plain ? (
                  <>
                    <label htmlFor={labelId} className="add-label">
                      {field.name}
                    </label>
                    <p className="note-editor-html-note" id={`${labelId}-note`}>
                      This field has formatting the editor cannot show.
                    </p>
                    <textarea
                      id={labelId}
                      className="note-editor-html"
                      aria-describedby={`${labelId}-note`}
                      value={values[field.id] ?? ""}
                      spellCheck={false}
                      autoCapitalize="off"
                      onChange={(e) => {
                        const html = e.target.value;
                        setValues((v) => ({ ...v, [field.id]: html }));
                      }}
                    />
                  </>
                ) : (
                  <>
                    <span id={labelId} className="add-label">
                      {field.name}
                    </span>
                    <FieldEditor
                      value={values[field.id] ?? ""}
                      onChange={(html) => setValues((v) => ({ ...v, [field.id]: html }))}
                      labelId={labelId}
                      cloze={isCloze}
                      highestCloze={highest}
                      {...editing.editorProps}
                      onReady={(view) => editing.ready(field.id, view)}
                    />
                  </>
                )}
                {index === 0 && duplicate && (
                  <p className="add-warning">
                    <WarningIcon />
                    <span>Another note with this {field.name.toLowerCase()} already exists.</span>
                  </p>
                )}
              </div>
            );
          })}

          <TagInput
            tags={tags}
            typed={typedTag}
            known={ready.knownTags}
            onTags={setTags}
            onTyped={setTypedTag}
          />

          <p role="status" className="add-status">
            {status}
          </p>
          {error && (
            <p role="alert" className="add-error">
              <WarningIcon />
              <span>{error}</span>
            </p>
          )}
          <button
            type="button"
            className="button button-danger-text note-editor-delete"
            disabled={pending}
            onClick={() => setAsking("delete")}
          >
            Delete note
          </button>

          <BottomAction>
            <div className="add-bar">
              <Toolbar
                view={editing.active}
                cloze={isCloze}
                highestCloze={highest}
                onFiles={(kind, files) => void editing.attach(kind, files)}
                busy={editing.mediaBusy}
              />
              <button
                type="button"
                className="button button-primary add-submit"
                disabled={pending || editing.mediaBusy}
                title="Save (Ctrl+S)"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => void save()}
              >
                Save
              </button>
            </div>
          </BottomAction>
        </form>
      )}
      {asking === "discard" && (
        <Dialog title="Discard your changes?" onClose={() => setAsking(null)}>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setAsking(null)}>
              Keep editing
            </button>
            <button type="button" className="button button-danger" onClick={onClose}>
              Discard
            </button>
          </div>
        </Dialog>
      )}
      {asking === "delete" && ready && (
        <Dialog
          title={`Delete this note and its ${cardsText(ready.note.cardCount)}?`}
          onClose={() => setAsking(null)}
        >
          <p>You can undo it right after.</p>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setAsking(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button-danger"
              disabled={pending}
              onClick={() => void remove()}
            >
              Delete
            </button>
          </div>
        </Dialog>
      )}
    </Dialog>
  );
}
