import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { BackIcon } from "../shell/icons";
import { BottomActionSlot } from "../shell/slot";

/** How many dialogs are mounted. Effects run twice in development, so a push is only undone once none is. */
let mounted = 0;

function inDialogEntry(): boolean {
  return Boolean(window.history.state?.fcDialog);
}

/**
 * A modal window on a native `dialog`, so focus and the backdrop just work. Mount it to open it
 * and unmount it to close it. `onClose` runs when the person dismisses it (Escape or the back
 * button); a dialog its parent keeps mounted stays open, so a window with unsaved changes can ask
 * first. It takes a history entry while open, so the Android back button closes the window and does
 * not leave the screen behind it.
 *
 * - `variant="sheet"` makes it a sheet from the bottom of the screen in the compact layout, for
 *   pickers in thumb reach.
 * - `variant="layer"` is a full-screen layer in the compact layout and a wide window from 600 px
 *   (the note editor, ADR 0014 decision 1), with a Back control and its own `BottomAction`, since
 *   the shell's is behind the modal.
 */
export function Dialog({
  title,
  onClose,
  children,
  variant,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  variant?: "sheet" | "layer";
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [slot, setSlot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
  }, []);

  useEffect(() => {
    if (!inDialogEntry()) window.history.pushState({ fcDialog: true }, "");
    mounted += 1;
    let alive = true;
    const onPop = () => {
      if (inDialogEntry()) return;
      onCloseRef.current();
      // Kept open (a question about unsaved changes): back needs an entry to close it again.
      setTimeout(() => {
        if (alive && !inDialogEntry()) window.history.pushState({ fcDialog: true }, "");
      }, 0);
    };
    window.addEventListener("popstate", onPop);
    return () => {
      alive = false;
      window.removeEventListener("popstate", onPop);
      mounted -= 1;
      // Closed by a button: take the entry back. Closed by back: it is already gone.
      setTimeout(() => {
        if (mounted === 0 && inDialogEntry()) window.history.back();
      }, 0);
    };
  }, []);

  const layer = variant === "layer";
  return (
    <dialog
      ref={ref}
      className={variant ? `dialog dialog-${variant}` : "dialog"}
      aria-labelledby={titleId}
      onCancel={(event) => {
        // Escape asks the parent, so the window does not close by itself.
        event.preventDefault();
        // WebKitGTK bubbles a window's cancel to the window it was opened over.
        if (event.target === event.currentTarget) onCloseRef.current();
      }}
      onClose={() => {
        onCloseRef.current();
        // The browser closed it anyway (some close a dialog on a second Escape): show it again if
        // the parent kept it.
        setTimeout(() => {
          const dialog = ref.current;
          if (dialog?.isConnected && !dialog.open && typeof dialog.showModal === "function") {
            dialog.showModal();
          }
        }, 0);
      }}
    >
      {layer ? (
        <>
          <div className="dialog-layer-head">
            <button
              type="button"
              className="dialog-layer-back"
              aria-label="Back"
              onClick={() => onCloseRef.current()}
            >
              <BackIcon />
            </button>
            <h2 id={titleId}>{title}</h2>
          </div>
          <div className="dialog-layer-body">
            <BottomActionSlot.Provider value={slot}>{children}</BottomActionSlot.Provider>
          </div>
          <div className="dialog-layer-action" ref={setSlot} />
        </>
      ) : (
        <>
          <h2 id={titleId}>{title}</h2>
          {children}
        </>
      )}
    </dialog>
  );
}
