import { type Command, EditorState, TextSelection } from "prosemirror-state";
import { describe, expect, test } from "vitest";
import {
  activeList,
  insertCloze,
  isMarkActive,
  toggleBold,
  toggleItalic,
  toggleList,
} from "./commands";
import { loadField, saveField } from "./html";
import { types } from "./schema";

/** A state for `html` with the text from `from` to `to` selected (document positions, 1 is the start of the text). */
function stateFor(html: string, from = 1, to = from): EditorState {
  const doc = loadField(html);
  const state = EditorState.create({ doc });
  return state.apply(state.tr.setSelection(TextSelection.create(doc, from, to)));
}

function run(command: Command, state: EditorState): EditorState {
  let result = state;
  const ok = command(state, (tr) => {
    result = state.apply(tr);
  });
  expect(ok).toBe(true);
  return result;
}

const html = (state: EditorState) => saveField(state.doc);

describe("insertCloze", () => {
  test("wraps the selection with the next number", () => {
    const state = run(
      insertCloze("new", () => 2),
      stateFor("Warszawa is big", 1, 9),
    );
    expect(html(state)).toBe("{{c3::Warszawa}} is big");
    // The selection still covers the word, so the next key replaces it or extends it.
    expect(state.doc.textBetween(state.selection.from, state.selection.to)).toBe("Warszawa");
  });

  test("is number 1 when no field has a cloze yet", () => {
    expect(
      html(
        run(
          insertCloze("new", () => 0),
          stateFor("kot", 1, 4),
        ),
      ),
    ).toBe("{{c1::kot}}");
  });

  test("the same number uses the highest, and 1 when there is none", () => {
    expect(
      html(
        run(
          insertCloze("same", () => 2),
          stateFor("a b", 3, 4),
        ),
      ),
    ).toBe("a {{c2::b}}");
    expect(
      html(
        run(
          insertCloze("same", () => 0),
          stateFor("a b", 3, 4),
        ),
      ),
    ).toBe("a {{c1::b}}");
  });

  test("with nothing selected it inserts an empty cloze with the cursor inside", () => {
    const state = run(
      insertCloze("new", () => 1),
      stateFor("ab", 2),
    );
    expect(html(state)).toBe("a{{c2::}}b");
    expect(state.selection.empty).toBe(true);
    expect(state.doc.textBetween(1, state.selection.from)).toBe("a{{c2::");
  });

  test("keeps formatting inside the selection", () => {
    const state = run(
      insertCloze("new", () => 0),
      stateFor("a <b>bold</b> z", 3, 7),
    );
    expect(html(state)).toBe("a {{c1::<b>bold</b>}} z");
  });

  test("takes the number when it is called, so a later edit in another field counts", () => {
    let highest = 1;
    const command = insertCloze("new", () => highest);
    expect(html(run(command, stateFor("x", 1, 2)))).toBe("{{c2::x}}");
    highest = 5;
    expect(html(run(command, stateFor("x", 1, 2)))).toBe("{{c6::x}}");
  });
});

describe("marks", () => {
  test("bold and italic toggle on a selection", () => {
    let state = run(toggleBold, stateFor("kot", 1, 4));
    expect(html(state)).toBe("<b>kot</b>");
    expect(isMarkActive(state, types.bold)).toBe(true);
    expect(isMarkActive(state, types.italic)).toBe(false);
    state = run(toggleItalic, state);
    expect(html(state)).toBe("<b><i>kot</i></b>");
    state = run(toggleBold, state);
    expect(html(state)).toBe("<i>kot</i>");
  });

  test("at the cursor the mark applies to what is typed next", () => {
    const state = run(toggleBold, stateFor("ab", 2));
    expect(isMarkActive(state, types.bold)).toBe(true);
    expect(state.storedMarks?.length).toBe(1);
  });
});

describe("lists", () => {
  test("a list is made from the line and taken off again", () => {
    let state = run(toggleList(types.bulletList), stateFor("one", 2));
    expect(html(state)).toBe("<ul><li>one</li></ul>");
    expect(activeList(state)).toBe(types.bulletList);
    state = run(toggleList(types.bulletList), state);
    expect(html(state)).toBe("one");
    expect(activeList(state)).toBeNull();
  });

  test("a numbered list is a different kind", () => {
    const state = run(toggleList(types.orderedList), stateFor("one", 2));
    expect(html(state)).toBe("<ol><li>one</li></ol>");
  });
});
