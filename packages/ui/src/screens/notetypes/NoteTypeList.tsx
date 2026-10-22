import { CoreError, type NoteTypeSummary } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { useCore } from "../../core";
import { Link, noteTypePath, PageHeading, useRouter } from "../../router";
import { BackIcon } from "../../shell/icons";
import "./notetypes.css";

type Loaded =
  | { status: "loading" }
  | { status: "failed"; message: string }
  | { status: "ready"; noteTypes: NoteTypeSummary[] };

const notesText = (n: number) => `${n} ${n === 1 ? "note" : "notes"}`;

/** Every note type, with a way to make a new one (step 3.4b). */
export function NoteTypeList() {
  const core = useCore();
  const { navigate } = useRouter();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [adding, setAdding] = useState(false);

  const load = useCallback(() => {
    setLoaded({ status: "loading" });
    core.call("getNoteTypes", null).then(
      (out) => setLoaded({ status: "ready", noteTypes: out.noteTypes }),
      (failure: unknown) =>
        setLoaded({
          status: "failed",
          message:
            failure instanceof CoreError ? failure.message : "The note types could not be read.",
        }),
    );
  }, [core]);
  useEffect(load, [load]);

  return (
    <>
      <button type="button" className="button back-link" onClick={() => navigate("/settings")}>
        <BackIcon />
        Settings
      </button>
      <PageHeading>Note types</PageHeading>
      {loaded.status === "loading" && <p role="status">Opening...</p>}
      {loaded.status === "failed" && (
        <div className="problem" role="alert">
          <p>{loaded.message}</p>
          <button type="button" className="button" onClick={load}>
            Try again
          </button>
        </div>
      )}
      {loaded.status === "ready" && (
        <>
          <p className="hint">
            A note type is the set of fields a note has and the cards made from them.
          </p>
          <ul className="notetype-list">
            {loaded.noteTypes.map((type) => (
              <li key={type.id}>
                <Link path={noteTypePath(type.id)} className="row-link notetype-row">
                  <span className="notetype-name">{type.name}</span>
                  <span className="notetype-meta">
                    {type.kind === "cloze" ? "Cloze" : "Standard"} · {notesText(type.noteCount)}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
          <button type="button" className="button button-primary" onClick={() => setAdding(true)}>
            New note type
          </button>
          {adding && (
            <NewNoteTypeDialog
              noteTypes={loaded.noteTypes}
              onClose={() => setAdding(false)}
              onCreated={(id) => navigate(noteTypePath(id))}
            />
          )}
        </>
      )}
    </>
  );
}

const SHAPE_PREFIX = "shape:";

/** Name, and what to start from: a copy of any note type, or the Basic or Cloze shape when none is left. */
function NewNoteTypeDialog({
  noteTypes,
  onClose,
  onCreated,
}: {
  noteTypes: NoteTypeSummary[];
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const core = useCore();
  const [name, setName] = useState("");
  const [from, setFrom] = useState(noteTypes[0]?.id ?? `${SHAPE_PREFIX}standard`);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);

  async function create() {
    if (busy.current) return;
    if (!name.trim()) {
      setError("The name is empty. Type a name.");
      return;
    }
    busy.current = true;
    setPending(true);
    setError(null);
    try {
      const shape = from.startsWith(SHAPE_PREFIX);
      const out = await core.call("createNoteType", {
        name,
        startFrom: shape ? null : from,
        kind: shape && from === `${SHAPE_PREFIX}cloze` ? "cloze" : "standard",
      });
      onCreated(out.noteTypeId);
    } catch (failure) {
      setError(
        failure instanceof CoreError ? failure.message : "The note type could not be added.",
      );
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  return (
    <Dialog title="New note type" variant="sheet" onClose={onClose}>
      <form
        className="dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <div className="dialog-field">
          <label htmlFor="new-type-name">Name</label>
          <input
            id="new-type-name"
            value={name}
            autoComplete="off"
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="dialog-field">
          <label htmlFor="new-type-from">Start from</label>
          <select id="new-type-from" value={from} onChange={(e) => setFrom(e.target.value)}>
            {noteTypes.length === 0 ? (
              <>
                <option value={`${SHAPE_PREFIX}standard`}>Basic: a front and a back</option>
                <option value={`${SHAPE_PREFIX}cloze`}>Cloze: gaps in a text</option>
              </>
            ) : (
              noteTypes.map((type) => (
                <option key={type.id} value={type.id}>
                  A copy of {type.name}
                </option>
              ))
            )}
          </select>
        </div>
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
            {pending ? "Adding..." : "Add"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
