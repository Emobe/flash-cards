import type { NoteTypeSummary } from "core-client";
import { describe, expect, test } from "vitest";
import { carries, defaultMaps, describeChange } from "./changeType";

const type = (
  id: string,
  kind: "standard" | "cloze",
  fields: string[],
  templates: string[],
): NoteTypeSummary => ({
  id,
  name: id,
  kind,
  fields: fields.map((name) => ({ id: `${id}/${name}`, name })),
  templates: templates.map((name) => ({ id: `${id}/t/${name}`, name })),
});

const basic = type("basic", "standard", ["Front", "Back"], ["Card 1"]);
const reversed = type("rev", "standard", ["Front", "Back"], ["Card 1", "Card 2"]);
const cloze = type("cloze", "cloze", ["Text", "Extra"], ["Cloze"]);
const other = type("other", "standard", ["Word", "Meaning", "Example"], ["Forward"]);

describe("defaultMaps", () => {
  test("matches by name ignoring case, then by position", () => {
    const upper = type("up", "standard", ["FRONT", "BACK"], ["CARD 1", "CARD 2"]);
    expect(defaultMaps(basic, upper).fields).toEqual({
      "up/FRONT": "basic/Front",
      "up/BACK": "basic/Back",
    });
    expect(defaultMaps(basic, other).fields).toEqual({
      "other/Word": "basic/Front",
      "other/Meaning": "basic/Back",
      "other/Example": "",
    });
  });

  test("gives each old card to one new template", () => {
    expect(defaultMaps(basic, reversed).templates).toEqual({
      "rev/t/Card 1": "basic/t/Card 1",
      "rev/t/Card 2": "",
    });
    // Position does not give the same old card to a second template.
    expect(defaultMaps(basic, other).templates).toEqual({ "other/t/Forward": "basic/t/Card 1" });
  });

  test("does not copy an old field that a name already took", () => {
    const swapped = type("sw", "standard", ["Back", "Notes"], ["Card 1"]);
    // Back is taken by name, and the field in Notes' position (Back) is already used.
    expect(defaultMaps(basic, swapped).fields).toEqual({
      "sw/Back": "basic/Back",
      "sw/Notes": "",
    });
  });

  test("cloze to cloze maps the one template", () => {
    const again = type("cloze2", "cloze", ["Text", "Extra"], ["Cloze 2"]);
    expect(defaultMaps(cloze, again).templates).toEqual({ "cloze2/t/Cloze 2": "cloze/t/Cloze" });
  });
});

describe("carries", () => {
  test("only between two standard types or two cloze types", () => {
    expect(carries(basic, reversed)).toBe(true);
    expect(carries(cloze, cloze)).toBe(true);
    expect(carries(basic, cloze)).toBe(false);
    expect(carries(cloze, basic)).toBe(false);
  });
});

describe("describeChange", () => {
  test("says what each card and field does", () => {
    expect(describeChange(12, basic, reversed, defaultMaps(basic, reversed))).toBe(
      "12 notes. Card 1 keeps its schedule. Card 2 starts as new.",
    );
    expect(describeChange(1, reversed, basic, defaultMaps(reversed, basic))).toBe(
      "1 note. Card 1 keeps its schedule.",
    );
  });

  test("names unused and empty fields, and renamed cards", () => {
    const maps = defaultMaps(basic, other);
    expect(describeChange(3, basic, other, maps)).toBe(
      "3 notes. Forward takes the schedule of Card 1. Example is left empty.",
    );
    const dropped = { ...maps, fields: { ...maps.fields, "other/Meaning": "" } };
    expect(describeChange(3, basic, other, dropped)).toContain(
      "Back is not used. Meaning and Example are left empty.",
    );
  });

  test("between kinds every card starts as new, and cloze keeps its numbers", () => {
    expect(describeChange(2, basic, cloze, defaultMaps(basic, cloze))).toContain(
      "Every card starts as new",
    );
    expect(describeChange(2, cloze, cloze, defaultMaps(cloze, cloze))).toContain(
      "keep their schedule by cloze number",
    );
  });
});
