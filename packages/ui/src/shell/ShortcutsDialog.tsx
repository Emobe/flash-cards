import { useEffect, useRef } from "react";
import { destinations } from "./destinations";

/** A modal list of the keyboard shortcuts. A native `dialog`, so focus and Escape just work. */
export function ShortcutsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!open && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    }
  }, [open]);

  return (
    <dialog ref={ref} className="shortcuts" aria-labelledby="shortcuts-title" onClose={onClose}>
      <h2 id="shortcuts-title">Keyboard shortcuts</h2>
      <dl>
        {destinations.map((d) => (
          <div key={d.id}>
            <dt>
              <kbd>g</kbd> then <kbd>{d.shortcut}</kbd>
            </dt>
            <dd>Go to {d.label}</dd>
          </div>
        ))}
        <div>
          <dt>
            <kbd>?</kbd>
          </dt>
          <dd>Show this list</dd>
        </div>
      </dl>
      <button type="button" onClick={onClose}>
        Close
      </button>
    </dialog>
  );
}
