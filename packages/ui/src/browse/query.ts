/**
 * Chips and the search box make one query text (ADR 0013 decision 1). `toQuery` writes the chips
 * in a fixed order and shape, then the box's text in brackets. `fromQuery` reads that shape back
 * from the front of a query and leaves anything else as text, so a query typed by hand opens with
 * all of it in the box. `query-fixtures.json` runs these outputs through the Rust search too.
 */

export type StateChoice = "new" | "learning" | "review" | "suspended" | "buried";
export type DueChoice = "now" | "overdue" | "tomorrow" | "week";
export type AddedChoice = "today" | "week" | "month";
export type AnsweredChoice = "today" | "again" | "week";

export type Filters = {
  /** Deck paths (`Polish::Verbs`). Each includes the decks inside it. */
  decks: string[];
  /** Tag names. Each includes the tags inside it. */
  tags: string[];
  noteTypes: string[];
  states: StateChoice[];
  due: DueChoice | null;
  added: AddedChoice | null;
  answered: AnsweredChoice | null;
};

export const noFilters: Filters = {
  decks: [],
  tags: [],
  noteTypes: [],
  states: [],
  due: null,
  added: null,
  answered: null,
};

export const stateChoices: StateChoice[] = ["new", "learning", "review", "suspended", "buried"];

export const dueTerms: Record<DueChoice, string> = {
  now: "is:due",
  overdue: "due:..-1",
  tomorrow: "due:1",
  week: "due:1..7",
};

export const addedTerms: Record<AddedChoice, string> = {
  today: "added:1",
  week: "added:7",
  month: "added:30",
};

export const answeredTerms: Record<AnsweredChoice, string> = {
  today: "rated:1",
  again: "rated:1:1",
  week: "rated:7",
};

/** A name in quotes, with `\`, `"` and `*` escaped, so it is never a wildcard. */
export function quote(name: string): string {
  return `"${name.replace(/[\\"*]/g, (c) => `\\${c}`)}"`;
}

function anyOf(terms: string[]): string | null {
  if (terms.length === 0) return null;
  return terms.length === 1 ? (terms[0] ?? null) : `(${terms.join(" or ")})`;
}

/** Whether `text` has anything to search for. */
export function hasText(text: string): boolean {
  return text.trim() !== "";
}

export function toQuery(filters: Filters, text: string): string {
  const parts = [
    anyOf(filters.decks.map((d) => `deck:=${quote(d)}`)),
    anyOf(filters.tags.map((t) => `tag:=${quote(t)}`)),
    anyOf(filters.noteTypes.map((n) => `note:=${quote(n)}`)),
    anyOf(filters.states.map((s) => `is:${s}`)),
    filters.due && dueTerms[filters.due],
    filters.added && addedTerms[filters.added],
    filters.answered && answeredTerms[filters.answered],
    hasText(text) ? `(${text})` : null,
  ];
  return parts.filter((p): p is string => p !== null).join(" ");
}

/** Reads the query from the front, one piece at a time. Every method gives null if it does not match. */
class Reader {
  at = 0;
  constructor(readonly query: string) {}

  skipSpace() {
    while (/\s/.test(this.query[this.at] ?? "")) this.at += 1;
  }

  /** The literal `s`, ending where a query word ends. */
  word(s: string): boolean {
    if (!this.query.startsWith(s, this.at)) return false;
    const next = this.query[this.at + s.length];
    if (next !== undefined && !/[\s)]/.test(next)) return false;
    this.at += s.length;
    return true;
  }

  /** `prefix` then a quoted name as `quote` writes it. */
  quoted(prefix: string): string | null {
    if (!this.query.startsWith(`${prefix}"`, this.at)) return null;
    let i = this.at + prefix.length + 1;
    let name = "";
    while (i < this.query.length) {
      const c = this.query[i];
      if (c === "\\") {
        const next = this.query[i + 1];
        if (next !== "\\" && next !== '"' && next !== "*") return null;
        name += next;
        i += 2;
      } else if (c === '"') {
        const after = this.query[i + 1];
        if (after !== undefined && !/[\s)]/.test(after)) return null;
        this.at = i + 1;
        return name;
      } else {
        name += c;
        i += 1;
      }
    }
    return null;
  }

  /** One `term`, or several in brackets joined by `or`. */
  anyOf<T>(term: () => T | null): T[] | null {
    const start = this.at;
    const one = term();
    if (one !== null) return [one];
    if (this.query[this.at] !== "(") return null;
    this.at += 1;
    const values: T[] = [];
    for (;;) {
      this.skipSpace();
      const value = term();
      if (value === null) break;
      values.push(value);
      this.skipSpace();
      if (this.query[this.at] === ")") {
        this.at += 1;
        if (values.length > 1) return values;
        break;
      }
      if (!this.word("or")) break;
    }
    this.at = start;
    return null;
  }

  oneOf<K extends string>(terms: Record<K, string>): K | null {
    for (const [key, term] of Object.entries(terms) as [K, string][]) {
      if (this.word(term)) return key;
    }
    return null;
  }

  /** The rest as one bracketed group: what is inside it, or null if the rest is anything else. */
  group(): string | null {
    if (this.query[this.at] !== "(") return null;
    let depth = 0;
    let inQuote = false;
    for (let i = this.at; i < this.query.length; i += 1) {
      const c = this.query[i];
      if (c === "\\" && (this.query[i + 1] === '"' || (inQuote && this.query[i + 1] === "\\"))) {
        i += 1;
      } else if (c === '"') {
        inQuote = !inQuote;
      } else if (!inQuote && c === "(") {
        depth += 1;
      } else if (!inQuote && c === ")") {
        depth -= 1;
        if (depth === 0) {
          if (this.query.slice(i + 1).trim() !== "") return null;
          return this.query.slice(this.at + 1, i);
        }
      }
    }
    return null;
  }
}

/**
 * The chips and text `toQuery` would have written for `query`. A query not in that shape opens
 * with everything as text, which finds the same cards.
 */
export function fromQuery(query: string): { filters: Filters; text: string } {
  const r = new Reader(query);
  const filters: Filters = { ...noFilters };
  const steps: (() => boolean)[] = [
    () =>
      set(
        "decks",
        r.anyOf(() => r.quoted("deck:=")),
      ),
    () =>
      set(
        "tags",
        r.anyOf(() => r.quoted("tag:=")),
      ),
    () =>
      set(
        "noteTypes",
        r.anyOf(() => r.quoted("note:=")),
      ),
    () =>
      set(
        "states",
        r.anyOf(() => stateChoices.find((s) => r.word(`is:${s}`)) ?? null),
      ),
    () => set("due", r.oneOf(dueTerms)),
    () => set("added", r.oneOf(addedTerms)),
    () => set("answered", r.oneOf(answeredTerms)),
  ];
  function set<K extends keyof Filters>(key: K, value: Filters[K] | null): boolean {
    if (value === null) return false;
    filters[key] = value;
    return true;
  }

  r.skipSpace();
  for (const step of steps) {
    if (step()) r.skipSpace();
  }
  if (r.at === query.length) return { filters, text: "" };
  const text = r.group();
  if (text !== null && hasText(text)) return { filters, text };
  return { filters: { ...noFilters }, text: query };
}
