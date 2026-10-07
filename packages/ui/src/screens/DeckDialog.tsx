import type { DeckSummary } from "core-client";
import { CoreError } from "core-client";
import { type FormEvent, useId, useState } from "react";
import { Dialog } from "../components/Dialog";

export type DeckAction =
  | { kind: "create"; parentId: string | null }
  | { kind: "rename"; deck: DeckSummary }
  | { kind: "move"; deck: DeckSummary }
  | { kind: "delete"; deck: DeckSummary };

/** The decks inside `deck`: in tree order they are the run of deeper decks right after it. */
export function decksInside(decks: DeckSummary[], deck: DeckSummary): DeckSummary[] {
  const start = decks.findIndex((d) => d.id === deck.id);
  const inside: DeckSummary[] = [];
  for (const d of decks.slice(start + 1)) {
    if (d.depth <= deck.depth) break;
    inside.push(d);
  }
  return inside;
}

/**
 * Naming, moving and deleting a deck (step 2.5a). One window for the four actions, so they look
 * and work the same on a phone and on a desktop. `onSubmit` rejects with a `CoreError` whose
 * message is shown in the window, which stays open so nothing typed is lost.
 */
export function DeckDialog({
  action,
  decks,
  onSubmit,
  onClose,
}: {
  action: DeckAction;
  decks: DeckSummary[];
  onSubmit: (values: { name: string; parentId: string | null }) => Promise<void>;
  onClose: () => void;
}) {
  const nameId = useId();
  const parentId = useId();
  const [name, setName] = useState(action.kind === "rename" ? action.deck.name : "");
  const [parent, setParent] = useState<string>(
    action.kind === "create"
      ? (action.parentId ?? "")
      : action.kind === "move"
        ? (action.deck.parentId ?? "")
        : "",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onSubmit({ name, parentId: parent === "" ? null : parent });
    } catch (e) {
      setError(
        e instanceof CoreError ? e.message : "Something went wrong. Close this and try again.",
      );
      setBusy(false);
    }
  }

  const title = {
    create: "Add deck",
    rename: "Rename deck",
    move: `Move "${action.kind === "move" ? action.deck.name : ""}"`,
    delete: "Delete deck",
  }[action.kind];

  let choices: DeckSummary[] = decks;
  if (action.kind === "move") {
    const blocked = new Set([action.deck.id, ...decksInside(decks, action.deck).map((d) => d.id)]);
    choices = decks.filter((d) => !blocked.has(d.id));
  }

  const inside = action.kind === "delete" ? decksInside(decks, action.deck) : [];

  return (
    <Dialog title={title} onClose={onClose}>
      <form onSubmit={submit} className="dialog-form">
        {action.kind === "delete" && (
          <p>
            Delete <strong>{action.deck.name}</strong>
            {inside.length > 0 &&
              ` and the ${inside.length === 1 ? "deck" : `${inside.length} decks`} inside it`}
            ? Its cards go too. You can undo this right after.
          </p>
        )}
        {(action.kind === "create" || action.kind === "rename") && (
          <div className="dialog-field">
            <label htmlFor={nameId}>Name</label>
            <input
              id={nameId}
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="off"
            />
          </div>
        )}
        {(action.kind === "create" || action.kind === "move") && (
          <div className="dialog-field">
            <label htmlFor={parentId}>{action.kind === "create" ? "Inside" : "Move to"}</label>
            <select id={parentId} value={parent} onChange={(e) => setParent(e.target.value)}>
              <option value="">Top level</option>
              {choices.map((d) => (
                <option key={d.id} value={d.id}>
                  {"  ".repeat(d.depth)}
                  {d.name}
                </option>
              ))}
            </select>
          </div>
        )}
        {error && (
          <p role="alert" className="dialog-error">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className={action.kind === "delete" ? "button button-danger" : "button button-primary"}
            disabled={busy}
          >
            {{ create: "Add", rename: "Rename", move: "Move", delete: "Delete" }[action.kind]}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
