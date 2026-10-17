import { describe, expect, test } from "vitest";
import {
  type AddedChoice,
  type AnsweredChoice,
  addedTerms,
  answeredTerms,
  type DueChoice,
  dueTerms,
  type Filters,
  fromQuery,
  noFilters,
  quote,
  stateChoices,
  toQuery,
} from "./query";
import fixtures from "./query-fixtures.json";

describe("the shared fixture", () => {
  for (const c of fixtures.cases) {
    const filters = { ...noFilters, ...c.filters } as Filters;
    test(c.about, () => {
      expect(toQuery(filters, c.text)).toBe(c.query);
      expect(fromQuery(c.query)).toEqual({ filters, text: c.text });
    });
  }
});

describe("quote", () => {
  test("escapes backslashes, quotes and stars", () => {
    expect(quote("plain")).toBe('"plain"');
    expect(quote('a"b*c\\')).toBe('"a\\"b\\*c\\\\"');
  });
});

describe("fromQuery", () => {
  test("a query typed by hand opens with everything as text", () => {
    for (const query of [
      "kot",
      "deck:Polish",
      'deck:="Verbs" kot',
      'deck:="Verbs" (kot) (pies)',
      'deck:="Verbs" or deck:="Polish"',
      'deck:="Verbs"x',
      'deck:="a\\b"',
      "is:new is:due added:1 rated:1:1 due:1",
      '-deck:="Verbs"',
      'deck:="Verbs" ()',
    ]) {
      expect(fromQuery(query), query).toEqual({ filters: noFilters, text: query });
    }
  });

  test("chips in their order and a bracketed text are read", () => {
    expect(fromQuery("is:new rated:1")).toEqual({
      filters: { ...noFilters, states: ["new"], answered: "today" },
      text: "",
    });
    expect(fromQuery('  tag:="a"   ( kot or "x)" )  ')).toEqual({
      filters: { ...noFilters, tags: ["a"] },
      text: ' kot or "x)" ',
    });
  });

  test("a query in one pair of brackets opens with what is inside", () => {
    expect(fromQuery('(deck:="Verbs" or tag:="x")')).toEqual({
      filters: noFilters,
      text: 'deck:="Verbs" or tag:="x"',
    });
  });

  test("a chip out of order leaves the whole query as text", () => {
    const query = 'tag:="a" deck:="b"';
    expect(fromQuery(query)).toEqual({ filters: noFilters, text: query });
  });
});

/** A small seeded generator, so a failure can be replayed. */
function random(seed: number) {
  let s = seed;
  const next = () => {
    s = (s * 1103515245 + 12345) % 2147483648;
    return s / 2147483648;
  };
  const pick = <T>(items: readonly T[]): T => items[Math.floor(next() * items.length)] as T;
  const some = <T>(items: readonly T[], max: number): T[] =>
    Array.from({ length: Math.floor(next() * (max + 1)) }, () => pick(items));
  return { next, pick, some };
}

const nameParts = [
  "a",
  "Polish",
  "Verbs",
  "::",
  '"',
  "*",
  "\\",
  " ",
  "ł",
  "ó",
  "ż",
  "(",
  ")",
  "or",
  "-",
  "=",
  ":",
];
const words = [
  "kot",
  "or",
  "and",
  "-pies",
  '"two words"',
  "(a or b)",
  "front:x*",
  "łódź",
  "deck:Polish",
];

function filtersFrom(r: ReturnType<typeof random>): Filters {
  const name = () => r.some(nameParts, 5).join("") || "x";
  const many = () => Array.from({ length: Math.floor(r.next() * 4) }, name);
  const maybe = <K extends string>(terms: Record<K, string>): K | null =>
    r.next() < 0.5 ? null : r.pick(Object.keys(terms) as K[]);
  return {
    decks: many(),
    tags: many(),
    noteTypes: many(),
    states: [...new Set(r.some(stateChoices, 3))],
    due: maybe<DueChoice>(dueTerms),
    added: maybe<AddedChoice>(addedTerms),
    answered: maybe<AnsweredChoice>(answeredTerms),
  };
}

test("fromQuery reads back whatever toQuery writes", () => {
  for (let seed = 1; seed <= 500; seed += 1) {
    const r = random(seed);
    const filters = filtersFrom(r);
    const text = r.some(words, 4).join(r.pick([" ", "  "]));
    const expected = { filters, text: text.trim() === "" ? "" : text };
    expect(fromQuery(toQuery(filters, text)), `seed ${seed}`).toEqual(expected);
  }
});
