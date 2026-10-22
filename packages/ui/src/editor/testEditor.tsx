import type { NoteDetails, NoteTypeSummary } from "core-client";
import { vi } from "vitest";

/**
 * A text area in place of `FieldEditor`, for screen tests: a real ProseMirror view cannot be typed
 * into under happy-dom. Use with `vi.mock("../editor/FieldEditor", () => import("../editor/testEditor"))`.
 */
export function FieldEditor({
  value,
  onChange,
  labelId,
}: {
  value: string;
  onChange: (html: string) => void;
  labelId: string;
}) {
  return (
    <textarea
      aria-labelledby={labelId}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

export const basic: NoteTypeSummary = {
  id: "nt-basic",
  name: "Basic",
  kind: "standard",
  noteCount: 0,
  fields: [
    { id: "f-front", name: "Front" },
    { id: "f-back", name: "Back" },
  ],
  templates: [{ id: "t-card-1", name: "Card 1" }],
};

export const editorDecks = [
  { id: "d-default", name: "Default", path: "Default", depth: 0 },
  { id: "d-polish", name: "Polish", path: "Polish", depth: 0 },
].map((d) => ({
  ...d,
  parentId: null,
  isDefault: d.id === "d-default",
  presetId: "p",
  newCount: 0,
  learningCount: 0,
  reviewCount: 0,
}));

/** The fake core methods the note editor calls, for a note with `fields` (front, back). */
export function editorHandlers(
  note: Partial<NoteDetails> & { front?: string; back?: string } = {},
) {
  const details: NoteDetails = {
    noteTypeId: "nt-basic",
    fields: [
      { fieldId: "f-front", value: note.front ?? "kot" },
      { fieldId: "f-back", value: note.back ?? "cat" },
    ],
    tags: ["animals"],
    deckIds: ["d-default"],
    cardCount: 1,
    ...note,
  };
  return {
    getNote: vi.fn((_input: { noteId: string }) => details),
    getNoteTypes: vi.fn(() => ({ noteTypes: [basic] })),
    getDeckList: vi.fn(() => ({ decks: editorDecks, totalCards: 1 })),
    getTags: vi.fn(() => ({ tags: ["animals", "pl"] })),
    findDuplicates: vi.fn((_input: { noteTypeId: string; value: string }) => ({
      noteIds: [] as string[],
    })),
    updateNote: vi.fn(
      (_input: {
        noteId: string;
        fields: { fieldId: string; value: string }[];
        tags: string[];
        deckId?: string;
      }) => ({
        cardsAdded: [] as string[],
        cardsRemoved: [] as string[],
        duplicates: [] as string[],
        undo: { id: "u1", message: "Saved." },
      }),
    ),
    deleteNotes: vi.fn((_input: { target: { notes: string[] } | { cards: string[] } }) => ({
      notes: 1,
      cards: 1,
      undo: { id: "u2", message: "Deleted 1 note and its 1 card." },
    })),
    undoChange: vi.fn((_input: { undoId: string }) => ({ message: "Undone." })),
  };
}
