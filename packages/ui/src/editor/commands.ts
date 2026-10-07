import { baseKeymap, chainCommands, toggleMark } from "prosemirror-commands";
import { history, redo, undo } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import type { MarkType, NodeType } from "prosemirror-model";
import { liftListItem, splitListItem, wrapInList } from "prosemirror-schema-list";
import {
  type Command,
  type EditorState,
  type Plugin,
  Selection,
  TextSelection,
} from "prosemirror-state";
import { schema, types } from "./schema";

const CLOZE_OPEN = /\{\{c(\d+)::/g;

/** The highest cloze number in any of these field HTML strings, or 0. */
export function highestCloze(fields: readonly string[]): number {
  let highest = 0;
  for (const field of fields) {
    for (const match of field.matchAll(CLOZE_OPEN)) {
      highest = Math.max(highest, Number(match[1]));
    }
  }
  return highest;
}

export type ClozeKind = "new" | "same";

/**
 * Wraps the selection in `{{cN::...}}`. `new` uses one more than the highest number in the note;
 * `same` uses the highest (1 when there is none yet). With nothing selected it inserts an empty
 * cloze and puts the cursor inside.
 */
export function insertCloze(kind: ClozeKind, highest: () => number): Command {
  return (state, dispatch) => {
    const top = highest();
    const number = kind === "new" ? top + 1 : Math.max(top, 1);
    const open = `{{c${number}::`;
    // Select all spans the whole document, not text: use the first and last place text can go.
    let { from, to } = state.selection;
    if (!state.doc.resolve(from).parent.inlineContent) {
      from = Selection.near(state.doc.resolve(from), 1).from;
    }
    if (!state.doc.resolve(to).parent.inlineContent) {
      to = Selection.near(state.doc.resolve(to), -1).to;
    }
    const empty = from >= to;
    if (dispatch) {
      const tr = state.tr;
      if (empty) {
        tr.insert(from, schema.text(`${open}}}`));
        tr.setSelection(TextSelection.create(tr.doc, from + open.length));
      } else {
        // The end first, so the start position is still right.
        // Plain text, whatever formatting the neighbours have, so the markers stay outside it.
        tr.insert(to, schema.text("}}"));
        tr.insert(from, schema.text(open));
        tr.setSelection(TextSelection.create(tr.doc, from + open.length, to + open.length));
      }
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}

export const toggleBold: Command = toggleMark(types.bold);
export const toggleItalic: Command = toggleMark(types.italic);

/** Whether the mark applies to the whole selection (or at the cursor). */
export function isMarkActive(state: EditorState, mark: MarkType): boolean {
  const { from, $from, to, empty } = state.selection;
  if (empty) return mark.isInSet(state.storedMarks ?? $from.marks()) !== undefined;
  return state.doc.rangeHasMark(from, to, mark);
}

/** The list type the selection is inside, if any. */
export function activeList(state: EditorState): NodeType | null {
  const { $from } = state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    const type = $from.node(depth).type;
    if (type === types.bulletList || type === types.orderedList) return type;
  }
  return null;
}

/** Makes a list of this type, or takes the selection out of it when it is already one. */
export function toggleList(type: NodeType): Command {
  return (state, dispatch, view) => {
    const current = activeList(state);
    const item = types.listItem;
    if (current === type) return liftListItem(item)(state, dispatch);
    if (current && dispatch && view) {
      // From one kind of list to the other: out, then in.
      liftListItem(item)(state, view.dispatch);
      return wrapInList(type)(view.state, view.dispatch);
    }
    return wrapInList(type)(state, dispatch);
  };
}

/** Enter makes a line break, not a new paragraph (storage rule, ADR 0011). */
const lineBreak: Command = (state, dispatch) => {
  if (dispatch) {
    dispatch(state.tr.replaceSelectionWith(types.hardBreak.create()).scrollIntoView());
  }
  return true;
};

export type EditorOptions = {
  /** Only a Cloze note type has the cloze shortcuts. */
  cloze: boolean;
  /** The highest cloze number across every field of the note, read when a shortcut is used. */
  highestCloze: () => number;
};

/**
 * Editing keys and history. Ctrl+Enter is left alone for the screen (it adds the note), and Tab
 * is left alone so the keyboard moves on to the next field.
 */
export function editorPlugins(options: EditorOptions): Plugin[] {
  const item = types.listItem;
  return [
    history(),
    keymap({
      "Mod-b": toggleBold,
      "Mod-i": toggleItalic,
      "Mod-z": undo,
      "Mod-y": redo,
      "Mod-Shift-z": redo,
      Enter: chainCommands(splitListItem(item), lineBreak),
      "Shift-Enter": lineBreak,
      ...(options.cloze
        ? {
            "Mod-Shift-c": insertCloze("new", options.highestCloze),
            "Mod-Alt-Shift-c": insertCloze("same", options.highestCloze),
          }
        : {}),
    }),
    keymap(baseKeymap),
  ];
}

/** Puts a picture or a sound, by its media name, at the selection. */
export function insertMedia(kind: "image" | "sound", name: string): Command {
  return (state, dispatch) => {
    const node = (kind === "image" ? types.image : types.sound).create({ name });
    if (dispatch) dispatch(state.tr.replaceSelectionWith(node).scrollIntoView());
    return true;
  };
}
