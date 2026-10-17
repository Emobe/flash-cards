import { useEffect, useRef, useState } from "react";
import { CheckIcon, CloseIcon } from "../shell/icons";
import { type ChipKind, chipDescription, chipKinds, chipLabel, chosen, type Names } from "./chips";
import type { Filters } from "./query";

/**
 * The filter chips, one row that scrolls sideways with a fade at the edge that has more (ADR 0013
 * decision 4). Saved, first, opens the saved searches. A filter chip opens its picker; one with
 * values shows a check mark and a × that clears it.
 */
export function ChipRow({
  filters,
  names,
  onSaved,
  onOpen,
  onClear,
}: {
  filters: Filters;
  names: Names | null;
  /** The Saved chip, first in the row: it opens the saved searches, it is not a filter. */
  onSaved: () => void;
  onOpen: (kind: ChipKind) => void;
  onClear: (kind: ChipKind) => void;
}) {
  const ref = useRef<HTMLFieldSetElement>(null);
  const [fade, setFade] = useState({ start: false, end: false });

  useEffect(() => {
    const row = ref.current;
    if (!row) return;
    const measure = () => {
      const start = row.scrollLeft > 1;
      const end = row.scrollLeft + row.clientWidth < row.scrollWidth - 1;
      setFade((f) => (f.start === start && f.end === end ? f : { start, end }));
    };
    measure();
    row.addEventListener("scroll", measure, { passive: true });
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(row);
    return () => {
      row.removeEventListener("scroll", measure);
      observer?.disconnect();
    };
  });

  return (
    <fieldset ref={ref} className="chip-row" data-fade-start={fade.start} data-fade-end={fade.end}>
      <legend className="visually-hidden">Filters</legend>
      <span className="chip">
        <button type="button" className="chip-main" aria-haspopup="dialog" onClick={onSaved}>
          <span>Saved</span>
        </button>
      </span>
      {chipKinds.map((kind) => {
        const values = chosen(filters, kind);
        const on = values.length > 0;
        return (
          <span key={kind} className="chip" data-on={on}>
            <button
              type="button"
              className="chip-main"
              aria-pressed={on}
              aria-label={chipDescription(kind, values, names)}
              onClick={() => onOpen(kind)}
            >
              {on && <CheckIcon />}
              <span>{chipLabel(kind, values, names)}</span>
            </button>
            {on && (
              <button
                type="button"
                className="chip-clear"
                aria-label={`Clear ${chipLabel(kind, [], names)}`}
                onClick={() => onClear(kind)}
              >
                <CloseIcon />
              </button>
            )}
          </span>
        );
      })}
    </fieldset>
  );
}
