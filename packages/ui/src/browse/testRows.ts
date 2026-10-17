import type { SearchResultRow } from "core-client";

/** A result row for tests: a new card in the Default deck unless `extra` says otherwise. */
export function resultRow(title: string, extra: Partial<SearchResultRow> = {}): SearchResultRow {
  return {
    cardId: `c-${title}`,
    noteId: `n-${title}`,
    noteTypeId: "nt-basic",
    deckId: "d-default",
    title,
    detail: "",
    state: "new",
    due: null,
    suspended: false,
    buried: false,
    ...extra,
  };
}
