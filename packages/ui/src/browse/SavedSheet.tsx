import { CoreError, type SavedSearchSummary } from "core-client";
import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { toQuery } from "./query";
import { type BrowseState, differsFrom } from "./useBrowseSearch";

/** A deleted saved search, kept so Undo can save it again. */
type Deleted = { search: SavedSearchSummary; wasOpen: boolean; error: string | null };

type Naming = { kind: "save" } | { kind: "rename"; search: SavedSearchSummary };

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

function message(error: unknown, fallback: string): string {
  return error instanceof CoreError ? error.message : fallback;
}

/**
 * The Saved chip's sheet (ADR 0013 decision 7): the saved searches by name, tap one to open it,
 * save what is on screen, update the open one, rename and delete with Undo. `searches` is the
 * screen's list; `reload` reads it again and answers once it has. A name the core refuses is shown
 * in the window, which stays open so nothing typed is lost.
 */
export function SavedSheet({
  state,
  searches,
  reload,
  onOpen,
  onOpened,
  onClose,
}: {
  state: BrowseState;
  searches: SavedSearchSummary[] | null;
  reload: () => Promise<void>;
  /** Shows a saved search on screen. */
  onOpen: (search: SavedSearchSummary) => void;
  /** Marks a saved search as the open one (or none), leaving the search on screen as it is. */
  onOpened: (id: string | null) => void;
  onClose: () => void;
}) {
  const core = useCore();
  const [naming, setNaming] = useState<Naming | null>(null);
  const [deleted, setDeleted] = useState<Deleted[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const open = searches?.find((s) => s.id === state.savedId) ?? null;
  const changed = open !== null && differsFrom(state, open);
  const current = {
    query: toQuery(state.filters, state.text),
    sort: state.sort,
    mode: state.mode,
  };

  /** Runs a write; a refusal shows in the window. */
  async function write(action: () => Promise<void>, fallback: string) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(message(e, fallback));
    } finally {
      setBusy(false);
    }
  }

  const update = (search: SavedSearchSummary) =>
    write(async () => {
      await core.call("updateSavedSearch", { id: search.id, name: null, ...current });
      await reload();
      onClose();
    }, "Could not update the saved search.");

  const remove = (search: SavedSearchSummary) =>
    write(async () => {
      await core.call("deleteSavedSearch", { id: search.id });
      const wasOpen = search.id === state.savedId;
      setDeleted((d) => [...d, { search, wasOpen, error: null }]);
      if (wasOpen) onOpened(null);
      await reload();
    }, "Could not delete the saved search.");

  async function undo(entry: Deleted) {
    const { name, query, sort, mode } = entry.search;
    try {
      const { id } = await core.call("saveSearch", { name, query, sort, mode });
      setDeleted((d) => d.filter((e) => e !== entry));
      // Read the list first, so the screen never sees an open ID that its list lacks.
      await reload();
      if (entry.wasOpen) onOpened(id);
    } catch (e) {
      const text = message(e, "Could not bring the saved search back.");
      setDeleted((d) => d.map((x) => (x === entry ? { ...x, error: text } : x)));
    }
  }

  if (naming) {
    return (
      <Dialog
        title={naming.kind === "save" ? "Save search" : "Rename saved search"}
        onClose={onClose}
        variant="sheet"
      >
        <NameForm
          initial={naming.kind === "rename" ? naming.search.name : ""}
          action={naming.kind === "save" ? "Save" : "Rename"}
          error={error}
          busy={busy}
          onCancel={() => {
            setError(null);
            setNaming(null);
          }}
          onSubmit={(name) =>
            naming.kind === "save"
              ? write(async () => {
                  const { id } = await core.call("saveSearch", { name, ...current });
                  await reload();
                  onOpened(id);
                  onClose();
                }, "Could not save the search.")
              : write(async () => {
                  await core.call("updateSavedSearch", {
                    id: naming.search.id,
                    name,
                    query: null,
                    sort: null,
                    mode: null,
                  });
                  await reload();
                  setNaming(null);
                }, "Could not rename the saved search.")
          }
        />
      </Dialog>
    );
  }

  const rows: (
    | { kind: "saved"; search: SavedSearchSummary }
    | { kind: "deleted"; entry: Deleted }
  )[] = [
    ...(searches ?? []).map((search) => ({ kind: "saved" as const, search })),
    ...deleted.map((entry) => ({ kind: "deleted" as const, entry })),
  ].sort((a, b) =>
    byName(
      a.kind === "saved" ? a.search : a.entry.search,
      b.kind === "saved" ? b.search : b.entry.search,
    ),
  );

  return (
    <Dialog title="Saved searches" onClose={onClose} variant="sheet">
      <div className="saved-sheet">
        {searches === null && <p role="status">Loading saved searches…</p>}
        {searches?.length === 0 && deleted.length === 0 && (
          <p className="picker-none">
            No saved searches yet. Save this search to come back to it later.
          </p>
        )}
        {rows.length > 0 && (
          <ul className="saved-list" aria-label="Saved searches">
            {rows.map((row) =>
              row.kind === "saved" ? (
                <li key={row.search.id} className="saved-row">
                  <button
                    type="button"
                    className="saved-open"
                    aria-current={row.search.id === state.savedId || undefined}
                    disabled={busy}
                    onClick={() => {
                      onOpen(row.search);
                      onClose();
                    }}
                  >
                    {row.search.name}
                    {row.search.id === state.savedId && (
                      <span className="saved-note">{changed ? "open, changed" : "open"}</span>
                    )}
                  </button>
                  <span className="saved-buttons">
                    <button
                      type="button"
                      className="button"
                      aria-label={`Rename ${row.search.name}`}
                      disabled={busy}
                      onClick={() => {
                        setError(null);
                        setNaming({ kind: "rename", search: row.search });
                      }}
                    >
                      Rename
                    </button>
                    <button
                      type="button"
                      className="button button-danger-text"
                      aria-label={`Delete ${row.search.name}`}
                      disabled={busy}
                      onClick={() => void remove(row.search)}
                    >
                      Delete
                    </button>
                  </span>
                </li>
              ) : (
                <li key={`deleted-${row.entry.search.id}`} className="saved-row" role="status">
                  <span className="saved-gone">
                    {row.entry.error ?? `Deleted "${row.entry.search.name}".`}
                  </span>
                  <button type="button" className="button" onClick={() => void undo(row.entry)}>
                    Undo
                  </button>
                </li>
              ),
            )}
          </ul>
        )}
        {error && (
          <p role="alert" className="dialog-error">
            {error}
          </p>
        )}
        <div className="dialog-actions saved-actions">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
          {open && changed ? (
            <>
              <button
                type="button"
                className="button"
                disabled={busy}
                onClick={() => {
                  setError(null);
                  setNaming({ kind: "save" });
                }}
              >
                Save as new
              </button>
              <button
                type="button"
                className="button button-primary"
                disabled={busy}
                onClick={() => void update(open)}
              >
                Update "{open.name}"
              </button>
            </>
          ) : (
            <button
              type="button"
              className="button button-primary"
              disabled={busy || searches === null}
              onClick={() => {
                setError(null);
                setNaming({ kind: "save" });
              }}
            >
              Save this search
            </button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

/** The name field for Save and Rename. */
function NameForm({
  initial,
  action,
  error,
  busy,
  onCancel,
  onSubmit,
}: {
  initial: string;
  action: string;
  error: string | null;
  busy: boolean;
  onCancel: () => void;
  onSubmit: (name: string) => void;
}) {
  const id = useId();
  const [name, setName] = useState(initial);
  const input = useRef<HTMLInputElement>(null);
  // The sheet stays open while it turns into this form, so nothing else moves focus here.
  useEffect(() => input.current?.focus(), []);
  return (
    <form
      className="dialog-form"
      onSubmit={(event: FormEvent) => {
        event.preventDefault();
        onSubmit(name);
      }}
    >
      <div className="dialog-field">
        <label htmlFor={id}>Name</label>
        <input
          id={id}
          ref={input}
          value={name}
          autoComplete="off"
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      {error && (
        <p role="alert" className="dialog-error">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="button" onClick={onCancel}>
          Back
        </button>
        <button type="submit" className="button button-primary" disabled={busy}>
          {action}
        </button>
      </div>
    </form>
  );
}
