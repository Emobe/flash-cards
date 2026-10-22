import type { NoteTypeDetails } from "core-client";
import { expect, test } from "vitest";
import { isChanged, nameProblem, newCardName, reducer, startState, toDraft } from "./draft";

const details: NoteTypeDetails = {
  id: "nt",
  kind: "standard",
  noteCount: 3,
  draft: {
    name: "Words",
    css: ".card {}",
    sortField: 0,
    fields: [
      { id: "f1", name: "Front" },
      { id: "f2", name: "Back" },
      { id: "f3", name: "Extra" },
    ],
    templates: [
      { id: "t1", name: "Card 1", front: "{{Front}}", back: "{{Back}}" },
      { id: "t2", name: "Card 2", front: "{{Back}}", back: "{{Front}}" },
    ],
  },
  removedFields: [{ id: "f9", name: "Old" }],
  removedTemplates: [],
};

const start = () => startState(details);
const names = (s: ReturnType<typeof start>) => s.fields.map((f) => f.name);

test("the start state gives back the draft it came from, and is not a change", () => {
  const s = start();
  expect(toDraft(s)).toEqual({ ...details.draft, sortField: 0 });
  expect(isChanged(s, start())).toBe(false);
});

test("moving a field up and down, and not past the ends", () => {
  let s = reducer(start(), { type: "move", list: "fields", key: "f3", by: -1 });
  expect(names(s)).toEqual(["Front", "Extra", "Back"]);
  s = reducer(s, { type: "move", list: "fields", key: "f1", by: -1 });
  expect(names(s)).toEqual(["Front", "Extra", "Back"]);
  s = reducer(s, { type: "move", list: "fields", key: "f3", by: 1 });
  expect(names(s)).toEqual(["Front", "Back", "Extra"]);
  expect(isChanged(s, start())).toBe(false);
});

test("the sort field follows its field when it moves, and falls to the first when removed", () => {
  let s = reducer(start(), { type: "sort", key: "f3" });
  s = reducer(s, { type: "move", list: "fields", key: "f3", by: -1 });
  expect(toDraft(s).sortField).toBe(1);
  s = reducer(s, { type: "remove", list: "fields", key: "f3" });
  expect(toDraft(s).sortField).toBe(0);
});

test("a removed field can be restored, and a new one removed again is gone", () => {
  let s = reducer(start(), { type: "remove", list: "fields", key: "f2" });
  expect(names(s)).toEqual(["Front", "Extra"]);
  expect(s.removedFields.map((f) => f.name)).toEqual(["Old", "Back"]);
  s = reducer(s, { type: "restore", list: "fields", key: "f2" });
  expect(names(s)).toEqual(["Front", "Extra", "Back"]);
  expect(toDraft(s).fields[2]).toEqual({ id: "f2", name: "Back" });

  s = reducer(s, { type: "addField", name: " Notes " });
  expect(s.fields[3]).toMatchObject({ id: null, name: "Notes" });
  s = reducer(s, { type: "remove", list: "fields", key: s.fields[3]?.key ?? "" });
  expect(s.removedFields.map((f) => f.name)).toEqual(["Old"]);
});

test("the last field and the last card cannot be removed", () => {
  let s = start();
  for (const key of ["f1", "f2"]) s = reducer(s, { type: "remove", list: "fields", key });
  s = reducer(s, { type: "remove", list: "fields", key: "f3" });
  expect(names(s)).toEqual(["Extra"]);
  s = reducer(s, { type: "remove", list: "templates", key: "t1" });
  s = reducer(s, { type: "remove", list: "templates", key: "t2" });
  expect(s.templates).toHaveLength(1);
});

test("a restore that would repeat a name does nothing", () => {
  let s = reducer(start(), { type: "addField", name: "old" });
  s = reducer(s, { type: "restore", list: "fields", key: "f9" });
  expect(s.removedFields).toHaveLength(1);
  expect(names(s)).toEqual(["Front", "Back", "Extra", "old"]);
});

test("renaming a field takes the rewritten templates", () => {
  const s = reducer(start(), {
    type: "renameField",
    key: "f1",
    name: " Word ",
    templates: [
      { id: "t1", name: "Card 1", front: "{{Word}}", back: "{{Back}}" },
      { id: "t2", name: "Card 2", front: "{{Back}}", back: "{{Word}}" },
    ],
  });
  expect(s.fields[0]?.name).toBe("Word");
  expect(s.templates[0]?.front).toBe("{{Word}}");
  expect(s.templates[1]?.back).toBe("{{Word}}");
  expect(isChanged(s, start())).toBe(true);
});

test("a new card copies the first, with the next free name", () => {
  expect(newCardName([{ name: "Card 1" }, { name: "Card 3" }])).toBe("Card 4");
  expect(newCardName([{ name: "Card 2" }])).toBe("Card 3");
  const s = reducer(start(), { type: "addTemplate", name: "Card 3" });
  expect(s.templates[2]).toMatchObject({
    id: null,
    name: "Card 3",
    front: "{{Front}}",
    back: "{{Back}}",
  });
});

test("names are checked for empty and for repeats ignoring case", () => {
  const fields = start().fields;
  expect(nameProblem(fields, "  ")).toMatch(/empty/);
  expect(nameProblem(fields, "back")).toMatch(/already one called "back"/);
  expect(nameProblem(fields, "Back", "f2")).toBeNull();
  expect(nameProblem(fields, "Fresh")).toBeNull();
});
