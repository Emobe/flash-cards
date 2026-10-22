/** Text edits for the template box (step 3.4c). Pure, so they are tested without a browser. */

export type Edit = { text: string; start: number; end: number };

/** Replaces the selection with `snippet` and puts the cursor after it. */
export function insertAt(text: string, start: number, end: number, snippet: string): Edit {
  const at = start + snippet.length;
  return { text: text.slice(0, start) + snippet + text.slice(end), start: at, end: at };
}

/**
 * Wraps the selection in `{{#Name}}` and `{{/Name}}`. With nothing selected the cursor goes between
 * the two, ready to type.
 */
export function wrapIf(text: string, start: number, end: number, name: string): Edit {
  const open = `{{#${name}}}`;
  const close = `{{/${name}}}`;
  const inner = text.slice(start, end);
  return {
    text: text.slice(0, start) + open + inner + close + text.slice(end),
    start: start + open.length,
    end: start + open.length + inner.length,
  };
}

/** The string offset of a place the core reports: line and column from 1, counted in characters. */
export function offsetOf(text: string, line: number, column: number): number {
  let at = 0;
  for (let l = 1; l < line; l++) {
    const next = text.indexOf("\n", at);
    if (next === -1) return text.length;
    at = next + 1;
  }
  const lineEnd = text.indexOf("\n", at);
  const rest = text.slice(at, lineEnd === -1 ? text.length : lineEnd);
  let offset = at;
  let chars = 1;
  for (const char of rest) {
    if (chars >= column) break;
    offset += char.length;
    chars += 1;
  }
  return offset;
}

/** The cloze numbers a note's field values use, from 1 up, once each. */
export function clozeNumbers(values: string[]): number[] {
  const found = new Set<number>();
  for (const value of values) {
    for (const match of value.matchAll(/\{\{c(\d+)::/g)) {
      const n = Number(match[1]);
      if (n >= 1 && Number.isSafeInteger(n)) found.add(n);
    }
  }
  return [...found].sort((a, b) => a - b);
}
