import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useWideLayout } from "../shell/useMediaQuery";

export type PickerOption = {
  value: string;
  label: string;
  /** How far to indent it (decks inside decks). */
  depth?: number;
  /** What the filter box matches, when not the label (a deck's whole path). */
  match?: string;
};

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/**
 * A chip's picker: checkboxes when several values can be chosen, radios when one, with Apply and
 * Clear (ADR 0013 decision 4). A sheet from the bottom on a phone. Back, Escape or Cancel close it
 * with nothing changed. A chosen value the collection no longer has is kept, marked "(not found)".
 * The selection bar's Move uses it too, with its own button and no Clear (step 3.2c).
 */
export function PickerDialog({
  title,
  multiple,
  options,
  selected,
  filterable,
  onApply,
  onClose,
  applyLabel = "Apply",
  clearable = true,
  findLabel = `Find a ${title.toLowerCase()}`,
  note,
  busy = false,
}: {
  title: string;
  multiple: boolean;
  options: PickerOption[];
  selected: string[];
  filterable: boolean;
  onApply: (values: string[]) => void;
  onClose: () => void;
  applyLabel?: string;
  clearable?: boolean;
  findLabel?: string;
  /** A line above the list (what Move acts on). */
  note?: ReactNode;
  /** Apply is off while the action runs. */
  busy?: boolean;
}) {
  const [picked, setPicked] = useState(() => new Set(selected));
  const [filter, setFilter] = useState("");
  const name = useId();
  const list = useRef<HTMLFieldSetElement>(null);
  const wide = useWideLayout();

  // The window focuses the filter box when it opens, which on a phone opens the keyboard over the
  // list. In the compact layout start on the first option instead; the filter is one tap away.
  // This runs after `Dialog`'s own effect (a child's effects run first).
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the window opens.
  useEffect(() => {
    if (!wide) list.current?.querySelector("input")?.focus();
  }, []);
  const known = new Set(options.map((o) => o.value));
  const all: PickerOption[] = [
    ...options,
    ...selected
      .filter((v) => !known.has(v))
      .map((v) => ({ value: v, label: `${v} (not found)`, match: v })),
  ];
  const wanted = fold(filter.trim());
  const shown = wanted ? all.filter((o) => fold(o.match ?? o.label).includes(wanted)) : all;

  function toggle(value: string, on: boolean) {
    if (!multiple) {
      setPicked(new Set([value]));
      return;
    }
    const next = new Set(picked);
    if (on) next.add(value);
    else next.delete(value);
    setPicked(next);
  }

  return (
    <Dialog title={title} onClose={onClose} variant="sheet">
      <form
        className="picker"
        onSubmit={(event) => {
          event.preventDefault();
          onApply(all.map((o) => o.value).filter((v) => picked.has(v)));
        }}
      >
        {note && <p className="picker-note">{note}</p>}
        {filterable && (
          <input
            type="search"
            className="picker-filter"
            aria-label={findLabel}
            placeholder="Find"
            value={filter}
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            enterKeyHint="done"
            onChange={(event) => setFilter(event.target.value)}
            onKeyDown={(event) => {
              // Enter in the filter box narrows the list, it does not apply.
              if (event.key === "Enter") event.preventDefault();
              // Escape clears the box, then closes the window. WebKitGTK's search field keeps an
              // Escape to itself even when empty, so the window would not close on its own.
              if (event.key === "Escape") {
                event.preventDefault();
                if (filter !== "") setFilter("");
                else onClose();
              }
            }}
          />
        )}
        <fieldset ref={list} className="picker-list">
          <legend className="visually-hidden">{title}</legend>
          {shown.length === 0 && <p className="picker-none">Nothing here matches.</p>}
          {shown.map((o) => (
            <label
              key={o.value}
              className="choice"
              style={{ "--depth": wanted ? 0 : (o.depth ?? 0) } as React.CSSProperties}
            >
              <input
                type={multiple ? "checkbox" : "radio"}
                name={name}
                checked={picked.has(o.value)}
                onChange={(event) => toggle(o.value, event.target.checked)}
              />
              <span>{wanted && o.match ? o.match : o.label}</span>
            </label>
          ))}
        </fieldset>
        <div className="dialog-actions">
          {clearable && (
            <button type="button" className="button" onClick={() => onApply([])}>
              Clear
            </button>
          )}
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="button button-primary"
            disabled={busy || (!clearable && picked.size === 0)}
          >
            {applyLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
