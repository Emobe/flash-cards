import { CoreError, type UndoToken } from "core-client";
import { useState } from "react";
import { useCore } from "../core";

/**
 * What a change did, with Undo and Dismiss (ADR 0014 decision 4). No timer: it stays until the
 * next change, leaving the screen, or Dismiss. Give it a `key` of the token's ID, so a new change
 * starts a new notice. `onUndone` runs after the undo was written, with what it did, so the screen
 * can read again. A screen that moves the notice (Browse, between layouts) passes that message back
 * as `undone`, so the notice does not offer Undo again.
 */
export function UndoNotice({
  token,
  undone,
  onUndone,
  onDismiss,
}: {
  token: UndoToken;
  undone?: string;
  onUndone: (message: string) => void;
  onDismiss: () => void;
}) {
  const core = useCore();
  const [message, setMessage] = useState(undone ?? token.message);
  const [state, setState] = useState<"ready" | "undoing" | "done">(
    undone === undefined ? "ready" : "done",
  );

  async function undo() {
    setState("undoing");
    try {
      const out = await core.call("undoChange", { undoId: token.id });
      setMessage(out.message);
      setState("done");
      onUndone(out.message);
    } catch (failure) {
      setMessage(failure instanceof CoreError ? failure.message : "That could not be undone.");
      setState("done");
    }
  }

  return (
    <div className="undo-notice" role="status">
      <span>{message}</span>
      {state !== "done" && (
        <button
          type="button"
          className="button"
          disabled={state === "undoing"}
          onClick={() => void undo()}
        >
          Undo
        </button>
      )}
      <button type="button" className="button" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
