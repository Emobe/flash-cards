import type { SearchDue, StudyCardState } from "core-client";
import { describe, expect, test } from "vitest";
import { formatDue, rowText } from "./format";

const row = (state: StudyCardState, due: SearchDue | null, suspended = false, buried = false) => ({
  state,
  due,
  suspended,
  buried,
});
const days = (n: number): SearchDue => ({ kind: "day", days: n });
const minutes = (n: number): SearchDue => ({ kind: "time", minutes: n });

describe("formatDue", () => {
  test("review cards by day", () => {
    expect(formatDue(row("review", days(0)))).toBe("Due today");
    expect(formatDue(row("review", days(1)))).toBe("Due tomorrow");
    expect(formatDue(row("review", days(3)))).toBe("Due in 3 days");
    expect(formatDue(row("review", days(-1)))).toBe("1 day overdue");
    expect(formatDue(row("review", days(-2)))).toBe("2 days overdue");
  });

  test("learning cards by time", () => {
    expect(formatDue(row("learning", minutes(1)))).toBe("In 1 minute");
    expect(formatDue(row("learning", minutes(12)))).toBe("In 12 minutes");
    expect(formatDue(row("relearning", minutes(60)))).toBe("In 1 hour");
    expect(formatDue(row("learning", minutes(150)))).toBe("In 3 hours");
    expect(formatDue(row("learning", minutes(0)))).toBe("Learning");
    expect(formatDue(row("learning", minutes(-10)))).toBe("Learning");
  });

  test("new cards", () => {
    expect(formatDue(row("new", null))).toBe("New");
  });

  test("suspended and buried come first", () => {
    expect(formatDue(row("review", days(-2), true))).toBe("Suspended");
    expect(formatDue(row("new", null, true, true))).toBe("Suspended");
    expect(formatDue(row("review", days(0), false, true))).toBe("Buried until tomorrow");
  });
});

describe("rowText", () => {
  test("a sound shows as a note sign", () => {
    expect(rowText("[sound:kot-0123456789abcdef.mp3]")).toBe("♪");
    expect(rowText("kot [sound:a.mp3] pies")).toBe("kot ♪ pies");
    expect(rowText("<b>not markup</b>")).toBe("<b>not markup</b>");
  });

  test("a cloze shows its answers without the markers", () => {
    expect(rowText("{{c1::Warszawa}} jest stolicą {{c2::Polski}}.")).toBe(
      "Warszawa jest stolicą Polski.",
    );
    expect(rowText("{{c1::kot::animal}} i {{C12::pies}}")).toBe("kot i pies");
    expect(rowText("{{c1::duży {{c2::kot}}}}")).toBe("duży kot");
    expect(rowText("{{c1::Warszawa}} [sound:a.mp3]")).toBe("Warszawa ♪");
    expect(rowText("{{Front}} and {{c1::}} and {{c1:x}}")).toBe("{{Front}} and  and {{c1:x}}");
  });
});
