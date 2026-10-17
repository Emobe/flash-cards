import type { NoteTypeSummary } from "core-client";

/** Three note types for the tests that change a note's type. */
export const typeBasic: NoteTypeSummary = {
  id: "nt-1",
  name: "Basic",
  kind: "standard",
  fields: [
    { id: "f1-front", name: "Front" },
    { id: "f1-back", name: "Back" },
  ],
  templates: [{ id: "t1-card-1", name: "Card 1" }],
};

export const typeReversed: NoteTypeSummary = {
  id: "nt-2",
  name: "Basic and reversed",
  kind: "standard",
  fields: [
    { id: "f2-front", name: "Front" },
    { id: "f2-back", name: "Back" },
  ],
  templates: [
    { id: "t2-card-1", name: "Card 1" },
    { id: "t2-card-2", name: "Card 2" },
  ],
};

export const typeCloze: NoteTypeSummary = {
  id: "nt-3",
  name: "Cloze",
  kind: "cloze",
  fields: [
    { id: "f3-text", name: "Text" },
    { id: "f3-extra", name: "Extra" },
  ],
  templates: [{ id: "t3-cloze", name: "Cloze" }],
};
