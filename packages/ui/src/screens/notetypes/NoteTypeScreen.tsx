import {
  type CheckNoteTypeOutput,
  CoreError,
  type NoteTypeDetails,
  type UndoToken,
} from "core-client";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { UndoNotice } from "../../components/UndoNotice";
import { useCore } from "../../core";
import { Link, PageHeading, useRouter } from "../../router";
import { BackIcon, WarningIcon } from "../../shell/icons";
import { BottomAction } from "../../shell/slot";
import { useBackEntry } from "../../shell/useBackEntry";
import { useWideLayout } from "../../shell/useMediaQuery";
import "../add/add.css";
import {
  type DraftState,
  isChanged,
  type List,
  nameProblem,
  newCardName,
  reducer,
  startState,
  toDraft,
} from "./draft";
import { type Tab, TemplateEditor } from "./TemplateEditor";
import "./notetypes.css";

const EMPTY: DraftState = {
  name: "",
  css: "",
  sortKey: null,
  fields: [],
  templates: [],
  removedFields: [],
  removedTemplates: [],
  next: 0,
};

type Loaded =
  | { status: "loading" }
  | { status: "failed"; message: string }
  | { status: "ready"; details: NoteTypeDetails; start: DraftState }
  /** Deleted from this screen: the notice offers Undo. */
  | { status: "deleted"; name: string; undo: UndoToken };

type Asking =
  | { kind: "options"; list: List; key: string }
  | { kind: "rename"; list: List; key: string }
  | { kind: "trash"; check: CheckNoteTypeOutput }
  | { kind: "delete" }
  | { kind: "discard" };

type Action = { label: string; run: () => void; disabled?: boolean; danger?: boolean };

function failureText(failure: unknown, fallback: string): string {
  return failure instanceof CoreError ? failure.message : fallback;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * One note type (step 3.4b, ADR 0015 decisions 2 and 5): its name, fields and cards are changed in
 * a draft and saved together with Save. A change that sends cards to the trash asks first. Edit
 * template and Styling open the template editor (3.4c) over it.
 */
export function NoteTypeScreen({ noteTypeId }: { noteTypeId: string }) {
  const core = useCore();
  const { navigate } = useRouter();
  const wide = useWideLayout();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [draft, dispatch] = useReducer(reducer, EMPTY);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ message: string; undo: UndoToken | null } | null>(null);
  /** The template editor over this screen, on a card (or the first one for Styling). */
  const [editing, setEditing] = useState<{ key: string; tab: Tab } | null>(null);
  const busy = useRef(false);

  const load = useCallback(() => {
    setLoaded({ status: "loading" });
    core.call("getNoteTypeDetails", { noteTypeId }).then(
      (details) => {
        const start = startState(details);
        dispatch({ type: "reset", state: start });
        setLoaded({ status: "ready", details, start });
      },
      (failure: unknown) =>
        setLoaded({
          status: "failed",
          message: failureText(failure, "The note type could not be read."),
        }),
    );
  }, [core, noteTypeId]);
  useEffect(load, [load]);

  const ready = loaded.status === "ready" ? loaded : null;
  const changed = ready !== null && isChanged(draft, ready.start);
  const cloze = ready?.details.kind === "cloze";

  // ---- Leaving ----

  function leave() {
    if (changed) setAsking({ kind: "discard" });
    else navigate("/notetypes");
  }
  const leaveRef = useRef(leave);
  leaveRef.current = leave;
  // The system Back asks too, instead of leaving.
  useBackEntry(changed, () => setAsking((now) => now ?? { kind: "discard" }));

  // ---- Saving ----

  async function save(confirmed = false) {
    if (!ready || busy.current || !changed) return;
    // Ctrl+S inside the template editor: the editor closes so the answer is seen here.
    setEditing(null);
    if (!draft.name.trim()) {
      setError("The name is empty. Type a name.");
      return;
    }
    busy.current = true;
    setPending(true);
    setError(null);
    try {
      const input = { noteTypeId, draft: toDraft(draft) };
      const check = await core.call("checkNoteType", input);
      if (check.cardsRemoved > 0 && !confirmed) {
        setAsking({ kind: "trash", check });
        return;
      }
      const out = await core.call("saveNoteType", input);
      setAsking(null);
      setNotice({ message: out.message, undo: out.undo });
      load();
    } catch (failure) {
      setAsking(null);
      setError(failureText(failure, "The note type could not be saved. Try again."));
    } finally {
      busy.current = false;
      setPending(false);
    }
  }
  const saveRef = useRef(save);
  saveRef.current = save;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveRef.current();
      } else if (event.key === "Escape" && !event.defaultPrevented) {
        // A window handles its own Escape.
        if (document.querySelector("dialog[open]")) return;
        leaveRef.current();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  // ---- Deleting ----

  async function remove() {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    try {
      const out = await core.call("deleteNoteType", { noteTypeId });
      setAsking(null);
      setLoaded({ status: "deleted", name: draft.name, undo: out.undo });
    } catch (failure) {
      setAsking(null);
      setError(failureText(failure, "The note type could not be deleted. Try again."));
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  // ---- Rows ----

  async function rename(list: List, key: string, name: string): Promise<string | null> {
    if (list === "templates") {
      const problem = nameProblem(draft.templates, name, key);
      if (problem) return problem;
      dispatch({ type: "renameTemplate", key, name });
      return null;
    }
    const problem = nameProblem(draft.fields, name, key);
    if (problem) return problem;
    const old = draft.fields.find((f) => f.key === key);
    if (!old) return null;
    try {
      const out = await core.call("renameFieldInTemplates", {
        templates: toDraft(draft).templates,
        from: old.name,
        to: name,
      });
      dispatch({ type: "renameField", key, name, templates: out.templates });
      return null;
    } catch (failure) {
      return failureText(failure, "The field could not be renamed.");
    }
  }

  function actionsFor(list: List, key: string): Action[] {
    const items = list === "fields" ? draft.fields : draft.templates;
    const index = items.findIndex((i) => i.key === key);
    const actions: Action[] =
      list === "templates"
        ? [{ label: "Edit template", run: () => setEditing({ key, tab: "front" }) }]
        : [];
    actions.push(
      { label: "Rename", run: () => setAsking({ kind: "rename", list, key }) },
      {
        label: "Move up",
        run: () => dispatch({ type: "move", list, key, by: -1 }),
        disabled: index <= 0,
      },
      {
        label: "Move down",
        run: () => dispatch({ type: "move", list, key, by: 1 }),
        disabled: index === items.length - 1,
      },
    );
    if (list === "fields") {
      actions.push({
        label: "Use to sort",
        run: () => dispatch({ type: "sort", key }),
        disabled: draft.sortKey === key,
      });
    }
    actions.push({
      label: "Remove",
      run: () => dispatch({ type: "remove", list, key }),
      disabled: items.length <= 1 || (list === "templates" && cloze),
      danger: true,
    });
    return actions;
  }

  function rows(list: List) {
    const items = list === "fields" ? draft.fields : draft.templates;
    return (
      <ul className="part-list">
        {items.map((item) => {
          const actions = actionsFor(list, item.key);
          return (
            <li key={item.key} className="part-row">
              <span className="part-name">{item.name}</span>
              {list === "fields" && draft.sortKey === item.key && (
                <span className="part-tag">Sorts notes</span>
              )}
              {wide ? (
                <span className="part-buttons">
                  {actions.map((action) => (
                    <button
                      key={action.label}
                      type="button"
                      className={action.danger ? "button button-danger-text" : "button"}
                      aria-label={`${action.label} ${item.name}`}
                      disabled={action.disabled}
                      onClick={action.run}
                    >
                      {action.label}
                    </button>
                  ))}
                </span>
              ) : (
                <button
                  type="button"
                  className="button"
                  aria-label={`Options for ${item.name}`}
                  onClick={() => setAsking({ kind: "options", list, key: item.key })}
                >
                  Options
                </button>
              )}
            </li>
          );
        })}
      </ul>
    );
  }

  function removed(list: List) {
    const items = list === "fields" ? draft.removedFields : draft.removedTemplates;
    if (items.length === 0) return null;
    const taken = list === "fields" ? draft.fields : draft.templates;
    return (
      <fieldset className="group">
        <legend>{list === "fields" ? "Removed fields" : "Removed cards"}</legend>
        <ul className="part-list">
          {items.map((item) => {
            const problem = nameProblem(taken, item.name);
            return (
              <li key={item.key} className="part-row">
                <span className="part-name">{item.name}</span>
                {problem && <span className="part-tag">A live one has this name</span>}
                <button
                  type="button"
                  className="button"
                  aria-label={`Restore ${item.name}`}
                  disabled={problem !== null}
                  onClick={() => dispatch({ type: "restore", list, key: item.key })}
                >
                  Restore
                </button>
              </li>
            );
          })}
        </ul>
      </fieldset>
    );
  }

  // ---- Render ----

  if (loaded.status === "deleted") {
    return (
      <>
        <PageHeading>{loaded.name}</PageHeading>
        <UndoNotice
          key={loaded.undo.id}
          token={loaded.undo}
          onUndone={() => load()}
          onDismiss={() => navigate("/notetypes")}
        />
        <Link path="/notetypes" className="back-link">
          Note types
        </Link>
      </>
    );
  }

  const options = asking?.kind === "options" ? asking : null;
  const renaming = asking?.kind === "rename" ? asking : null;
  const optionName = (list: List, key: string) =>
    (list === "fields" ? draft.fields : draft.templates).find((i) => i.key === key)?.name ?? "";

  return (
    <div className="notetype-screen">
      <button type="button" className="button back-link" onClick={leave}>
        <BackIcon />
        Note types
      </button>
      <PageHeading>{ready ? ready.start.name || "Note type" : "Note type"}</PageHeading>
      {loaded.status === "loading" && <p role="status">Opening...</p>}
      {loaded.status === "failed" && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{loaded.message}</p>
          <button type="button" className="button" onClick={load}>
            Try again
          </button>
        </div>
      )}
      {notice &&
        (notice.undo ? (
          <UndoNotice
            key={notice.undo.id}
            token={notice.undo}
            onUndone={() => load()}
            onDismiss={() => setNotice(null)}
          />
        ) : (
          <p role="status">{notice.message}</p>
        ))}
      {ready && (
        <form aria-label="Edit the note type" onSubmit={(event) => event.preventDefault()}>
          <div className="notetype-screen">
            <div className="add-field">
              <label htmlFor="notetype-name" className="add-label">
                Name
              </label>
              <input
                id="notetype-name"
                value={draft.name}
                autoComplete="off"
                onChange={(e) => dispatch({ type: "name", name: e.target.value })}
              />
              <span className="hint">
                {ready.details.kind === "cloze" ? "Cloze" : "Standard"} ·{" "}
                {plural(ready.details.noteCount, "note", "notes")}
              </span>
            </div>

            <fieldset className="group">
              <legend>Fields</legend>
              {rows("fields")}
              <AddPart
                label="Field name"
                button="Add field"
                problem={(name) => nameProblem(draft.fields, name)}
                onAdd={(name) => dispatch({ type: "addField", name })}
              />
            </fieldset>
            {removed("fields")}

            <fieldset className="group">
              <legend>Cards</legend>
              {cloze && (
                <p className="hint">A cloze note type has one card, made for each gap in a note.</p>
              )}
              {rows("templates")}
              <button
                type="button"
                className="button"
                disabled={!draft.templates[0]}
                onClick={() => {
                  const first = draft.templates[0];
                  if (first) setEditing({ key: first.key, tab: "styling" });
                }}
              >
                Styling
              </button>
              {!cloze && (
                <AddPart
                  label="Card name"
                  button="Add card"
                  initial={newCardName(draft.templates)}
                  problem={(name) => nameProblem(draft.templates, name)}
                  onAdd={(name) => dispatch({ type: "addTemplate", name })}
                />
              )}
            </fieldset>
            {removed("templates")}

            <p role="status" className="add-status">
              {pending ? "Saving..." : ""}
            </p>
            {error && (
              <p role="alert" className="notetype-error">
                <WarningIcon />
                <span>{error}</span>
              </p>
            )}
            <button
              type="button"
              className="button button-danger-text"
              disabled={pending}
              onClick={() => setAsking({ kind: "delete" })}
            >
              Delete note type
            </button>
          </div>
          <BottomAction>
            <div className="add-bar">
              <button
                type="button"
                className="button button-primary add-submit"
                disabled={pending || !changed}
                title="Save (Ctrl+S)"
                onClick={() => void save()}
              >
                {pending ? "Saving..." : "Save"}
              </button>
            </div>
          </BottomAction>
        </form>
      )}

      {editing && ready && (
        <TemplateEditor
          noteTypeId={noteTypeId}
          cloze={cloze === true}
          draft={draft}
          dispatch={dispatch}
          templateKey={editing.key}
          initialTab={editing.tab}
          onClose={() => setEditing(null)}
        />
      )}

      {options && (
        <Dialog
          title={optionName(options.list, options.key)}
          variant="sheet"
          onClose={() => setAsking(null)}
        >
          <div className="part-sheet">
            {actionsFor(options.list, options.key).map((action) => (
              <button
                key={action.label}
                type="button"
                className={action.danger ? "button button-danger-text" : "button"}
                disabled={action.disabled}
                onClick={() => {
                  // Rename opens its own window; the others act and close.
                  if (action.label === "Rename") action.run();
                  else {
                    setAsking(null);
                    action.run();
                  }
                }}
              >
                {action.label}
              </button>
            ))}
          </div>
        </Dialog>
      )}
      {renaming && (
        <RenameDialog
          title={`Rename ${optionName(renaming.list, renaming.key)}`}
          initial={optionName(renaming.list, renaming.key)}
          note={
            renaming.list === "fields" ? "Cards that use this field are rewritten to match." : null
          }
          onApply={(name) => rename(renaming.list, renaming.key, name)}
          onClose={() => setAsking(null)}
        />
      )}
      {asking?.kind === "trash" && (
        <Dialog title="Send cards to the trash?" onClose={() => setAsking(null)}>
          <p>
            Saving sends {plural(asking.check.cardsRemoved, "card", "cards")} of{" "}
            {plural(asking.check.notesAffected, "note", "notes")} to the trash, because the cards
            would be empty. You can undo it right after, and the cards come back with their history.
          </p>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setAsking(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="button button-danger"
              disabled={pending}
              onClick={() => void save(true)}
            >
              Save
            </button>
          </div>
        </Dialog>
      )}
      {asking?.kind === "discard" && (
        <Dialog title="Discard your changes?" onClose={() => setAsking(null)}>
          <div className="dialog-actions">
            <button type="button" className="button" onClick={() => setAsking(null)}>
              Keep editing
            </button>
            <button
              type="button"
              className="button button-danger"
              onClick={() => navigate("/notetypes")}
            >
              Discard
            </button>
          </div>
        </Dialog>
      )}
      {asking?.kind === "delete" && ready && (
        <Dialog
          title={`Delete ${ready.start.name} and its ${plural(ready.details.noteCount, "note", "notes")}?`}
          onClose={() => setAsking(null)}
        >
          <p>The notes and their cards go to the trash with it. You can undo it right after.</p>
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
    </div>
  );
}

/** A name box and a button that adds an item with that name. */
function AddPart({
  label,
  button,
  initial = "",
  problem,
  onAdd,
}: {
  label: string;
  button: string;
  initial?: string;
  problem: (name: string) => string | null;
  onAdd: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  function add() {
    const wanted = name.trim() || initial;
    const why = problem(wanted);
    if (why) {
      setError(why);
      return;
    }
    onAdd(wanted);
    setName("");
    setError(null);
  }
  return (
    <>
      <div className="part-add">
        <input
          aria-label={label}
          placeholder={initial || label}
          value={name}
          autoComplete="off"
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <button type="button" className="button" onClick={add}>
          {button}
        </button>
      </div>
      {error && (
        <p role="alert" className="notetype-error">
          <WarningIcon />
          <span>{error}</span>
        </p>
      )}
    </>
  );
}

function RenameDialog({
  title,
  initial,
  note,
  onApply,
  onClose,
}: {
  title: string;
  initial: string;
  note: string | null;
  onApply: (name: string) => Promise<string | null>;
  onClose: () => void;
}) {
  const [name, setName] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  async function apply() {
    setPending(true);
    const why = await onApply(name);
    setPending(false);
    if (why) setError(why);
    else onClose();
  }
  return (
    <Dialog title={title} variant="sheet" onClose={onClose}>
      <form
        className="dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          void apply();
        }}
      >
        <div className="dialog-field">
          <label htmlFor="rename-name">Name</label>
          <input
            id="rename-name"
            value={name}
            autoComplete="off"
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        {note && <p className="hint">{note}</p>}
        {error && (
          <p role="alert" className="dialog-error">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={pending}>
            Rename
          </button>
        </div>
      </form>
    </Dialog>
  );
}
