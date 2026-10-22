import { describe, expect, test } from "vitest";
import { clozeNumbers, insertAt, offsetOf, wrapIf } from "./templateText";

describe("insertAt", () => {
  test("puts the snippet at the cursor", () => {
    expect(insertAt("ab", 1, 1, "{{X}}")).toEqual({ text: "a{{X}}b", start: 6, end: 6 });
  });
  test("replaces a selection", () => {
    expect(insertAt("abc", 0, 2, "Z").text).toBe("Zc");
  });
});

describe("wrapIf", () => {
  test("wraps the selection", () => {
    const edit = wrapIf("a hello b", 2, 7, "Note");
    expect(edit.text).toBe("a {{#Note}}hello{{/Note}} b");
    expect(edit.text.slice(edit.start, edit.end)).toBe("hello");
  });
  test("with nothing selected the cursor sits between the tags", () => {
    const edit = wrapIf("", 0, 0, "Note");
    expect(edit).toEqual({ text: "{{#Note}}{{/Note}}", start: 9, end: 9 });
  });
});

describe("offsetOf", () => {
  test("counts lines and columns from 1", () => {
    expect(offsetOf("ab\ncd\nef", 2, 2)).toBe(4);
    expect(offsetOf("ab\ncd", 1, 1)).toBe(0);
  });
  test("stays on the line and in the text", () => {
    expect(offsetOf("ab\ncd", 1, 99)).toBe(2);
    expect(offsetOf("ab", 5, 1)).toBe(2);
  });
  test("counts characters, not UTF-16 units", () => {
    expect(offsetOf("😀x", 1, 2)).toBe(2);
  });
});

describe("clozeNumbers", () => {
  test("lists each number once, in order", () => {
    expect(clozeNumbers(["{{c2::a}} {{c1::b}}", "{{c2::c}}"])).toEqual([1, 2]);
  });
  test("none", () => {
    expect(clozeNumbers(["plain"])).toEqual([]);
  });
});
