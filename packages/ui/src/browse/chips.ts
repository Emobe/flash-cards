import type { DeckSummary } from "core-client";
import type { AddedChoice, AnsweredChoice, DueChoice, Filters, StateChoice } from "./query";

/** The filter chips, in order (ADR 0013 decision 4). The Saved chip comes before them. */
export const chipKinds = [
  "decks",
  "tags",
  "noteTypes",
  "states",
  "due",
  "added",
  "answered",
] as const satisfies readonly (keyof Filters)[];
export type ChipKind = (typeof chipKinds)[number];

export const chipNames: Record<ChipKind, string> = {
  decks: "Deck",
  tags: "Tag",
  noteTypes: "Note type",
  states: "State",
  due: "Due",
  added: "Added",
  answered: "Answered",
};

export const stateLabels: Record<StateChoice, string> = {
  new: "New",
  learning: "Learning",
  review: "Review",
  suspended: "Suspended",
  buried: "Buried",
};

export const dueLabels: Record<DueChoice, string> = {
  now: "Due now",
  overdue: "Overdue",
  tomorrow: "Due tomorrow",
  week: "Due in the next 7 days",
};

export const addedLabels: Record<AddedChoice, string> = {
  today: "Today",
  week: "Last 7 days",
  month: "Last 30 days",
};

export const answeredLabels: Record<AnsweredChoice, string> = {
  today: "Today",
  again: "Answered Again today",
  week: "Last 7 days",
};

/** Several values are "any of", so a chip of that kind is a list of checkboxes. */
export const multiple: Record<ChipKind, boolean> = {
  decks: true,
  tags: true,
  noteTypes: true,
  states: true,
  due: false,
  added: false,
  answered: false,
};

/** The deck, tag and note type names the pickers offer, read from the core. */
export type Names = {
  decks: DeckSummary[];
  tags: string[];
  noteTypes: string[];
  /** Cards in the whole collection, to tell an empty collection from a search with no match. */
  totalCards: number;
};

export function chosen(filters: Filters, kind: ChipKind): string[] {
  const value = filters[kind];
  if (Array.isArray(value)) return value;
  return value === null ? [] : [value];
}

export function withChosen(filters: Filters, kind: ChipKind, values: string[]): Filters {
  return { ...filters, [kind]: multiple[kind] ? values : (values[0] ?? null) };
}

const fold = (s: string) => s.toLowerCase();

/**
 * Whether a chosen name is gone from the collection (a renamed or deleted deck, tag or note type).
 * Unknown until the names have been read. Names match ignoring case, as the search does.
 */
export function isMissing(kind: ChipKind, value: string, names: Names | null): boolean {
  if (!names) return false;
  const v = fold(value);
  if (kind === "decks") return !names.decks.some((d) => fold(d.path) === v);
  if (kind === "tags") return !names.tags.some((t) => fold(t) === v);
  if (kind === "noteTypes") return !names.noteTypes.some((n) => fold(n) === v);
  return false;
}

/** How one chosen value reads: a deck by its own name, a choice by its label. */
export function valueLabel(kind: ChipKind, value: string): string {
  switch (kind) {
    case "decks":
      return value.split("::").at(-1) ?? value;
    case "states":
      return stateLabels[value as StateChoice];
    case "due":
      return dueLabels[value as DueChoice];
    case "added":
      return addedLabels[value as AddedChoice];
    case "answered":
      return answeredLabels[value as AnsweredChoice];
    default:
      return value;
  }
}

/** The chip's text: "Deck", "Deck: Verbs", "Tag: 2". */
export function chipLabel(kind: ChipKind, values: string[], names: Names | null): string {
  const [only] = values;
  if (only === undefined) return chipNames[kind];
  if (values.length > 1) return `${chipNames[kind]}: ${values.length}`;
  const missing = isMissing(kind, only, names) ? " (not found)" : "";
  return `${chipNames[kind]}: ${valueLabel(kind, only)}${missing}`;
}

/** What a screen reader hears: every chosen value, a deck by its whole path. */
export function chipDescription(kind: ChipKind, values: string[], names: Names | null): string {
  if (values.length === 0) return `${chipNames[kind]}: any`;
  const parts = values.map((v) => {
    const text = kind === "decks" ? v : valueLabel(kind, v);
    return isMissing(kind, v, names) ? `${text} (not found)` : text;
  });
  return `${chipNames[kind]}: ${parts.join(", ")}`;
}
