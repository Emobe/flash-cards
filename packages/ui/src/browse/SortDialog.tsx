import type { SearchMode, SearchSort, SearchSortKey } from "core-client";
import { Dialog } from "../components/Dialog";
import { sortKeys } from "./useBrowseSearch";

/** Each sort in plain words, its default direction, and how each direction reads (ADR 0013 decision 6). */
const sorts: Record<
  SearchSortKey,
  { label: string; descending: boolean; ascendingText: string; descendingText: string }
> = {
  created: {
    label: "Date added",
    descending: true,
    ascendingText: "oldest first",
    descendingText: "newest first",
  },
  due: {
    label: "Due date",
    descending: false,
    ascendingText: "soonest first",
    descendingText: "latest first",
  },
  field: {
    label: "Alphabetical",
    descending: false,
    ascendingText: "A to Z",
    descendingText: "Z to A",
  },
  deck: {
    label: "Deck",
    descending: false,
    ascendingText: "deck list order",
    descendingText: "reverse deck list order",
  },
  note: {
    label: "Note type",
    descending: false,
    ascendingText: "A to Z",
    descendingText: "Z to A",
  },
  difficulty: {
    label: "Difficulty",
    descending: true,
    ascendingText: "easiest first",
    descendingText: "hardest first",
  },
  stability: {
    label: "How well you know it",
    descending: false,
    ascendingText: "weakest first",
    descendingText: "strongest first",
  },
  lapses: {
    label: "Times forgotten",
    descending: true,
    ascendingText: "fewest first",
    descendingText: "most first",
  },
  reviews: {
    label: "Times reviewed",
    descending: true,
    ascendingText: "fewest first",
    descendingText: "most first",
  },
  random: {
    label: "Shuffle",
    descending: false,
    ascendingText: "a new order each time",
    descendingText: "a new order each time",
  },
};

export function sortLabel(sort: SearchSort): string {
  const s = sorts[sort.key];
  return `${s.label}, ${sort.descending ? s.descendingText : s.ascendingText}`;
}

/**
 * The Sort button's window: the ten sorts, a second tap on the chosen one reverses it, Shuffle
 * takes a new order each time it is tapped, and the Notes or Cards switch. Changes apply at once.
 */
export function SortDialog({
  sort,
  mode,
  onSort,
  onMode,
  onClose,
}: {
  sort: SearchSort;
  mode: SearchMode;
  onSort: (sort: SearchSort) => void;
  onMode: (mode: SearchMode) => void;
  onClose: () => void;
}) {
  return (
    <Dialog title="Sort and show" onClose={onClose} variant="sheet">
      <div className="sort-dialog">
        <fieldset className="segmented">
          <legend className="visually-hidden">Show</legend>
          <span className="segmented-label" aria-hidden="true">
            Show
          </span>
          {(["notes", "cards"] as const).map((m) => (
            <button
              key={m}
              type="button"
              className="button"
              aria-pressed={mode === m}
              onClick={() => onMode(m)}
            >
              {m === "notes" ? "Notes" : "Cards"}
            </button>
          ))}
        </fieldset>
        <ul className="sort-list" aria-label="Sort by">
          {sortKeys.map((key) => {
            const current = sort.key === key;
            const s = sorts[key];
            const descending = current ? sort.descending : s.descending;
            return (
              <li key={key}>
                <button
                  type="button"
                  className="sort-option"
                  aria-pressed={current}
                  onClick={() =>
                    onSort({
                      key,
                      descending: current && key !== "random" ? !sort.descending : s.descending,
                    })
                  }
                >
                  <span>{s.label}</span>
                  <span className="sort-direction">
                    {descending ? s.descendingText : s.ascendingText}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        <div className="dialog-actions">
          <button type="button" className="button button-primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </Dialog>
  );
}
