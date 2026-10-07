import { type ReactNode, useEffect, useId, useRef } from "react";

/** How many dialogs are mounted. Effects run twice in development, so a push is only undone once none is. */
let mounted = 0;

function inDialogEntry(): boolean {
  return Boolean(window.history.state?.fcDialog);
}

/**
 * A modal window on a native `dialog`, so focus, Escape and the backdrop just work. Mount it to
 * open it and unmount it to close it. `onClose` runs when the person dismisses it (Escape or the
 * back button). It takes a history entry while open, so the Android back button closes the window
 * and does not leave the screen behind it.
 */
export function Dialog({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
  }, []);

  useEffect(() => {
    if (!inDialogEntry()) window.history.pushState({ fcDialog: true }, "");
    mounted += 1;
    const onPop = () => {
      if (!inDialogEntry()) onCloseRef.current();
    };
    window.addEventListener("popstate", onPop);
    return () => {
      window.removeEventListener("popstate", onPop);
      mounted -= 1;
      // Closed by a button: take the entry back. Closed by back: it is already gone.
      setTimeout(() => {
        if (mounted === 0 && inDialogEntry()) window.history.back();
      }, 0);
    };
  }, []);

  return (
    <dialog ref={ref} className="dialog" aria-labelledby={titleId} onClose={onClose}>
      <h2 id={titleId}>{title}</h2>
      {children}
    </dialog>
  );
}
