import type { FieldDraft, NoteTypeDetails, NoteTypeDraft, TemplateDraft } from "core-client";

/**
 * The note type being edited, held until Save (ADR 0015 decision 2). Every item has a `key` that
 * does not change while the screen is open, so a new item (no ID yet) can be told apart and moved.
 */
export type FieldItem = { key: string; id: string | null; name: string };
export type TemplateItem = {
  key: string;
  id: string | null;
  name: string;
  front: string;
  back: string;
};

export type DraftState = {
  name: string;
  css: string;
  /** The key of the field that sorts and names notes. */
  sortKey: string | null;
  fields: FieldItem[];
  templates: TemplateItem[];
  /** Fields and templates that are out of the note type, and can come back. */
  removedFields: FieldItem[];
  removedTemplates: TemplateItem[];
  /** Counts keys for items that are new. */
  next: number;
};

export type List = "fields" | "templates";

export type Action =
  | { type: "reset"; state: DraftState }
  | { type: "name"; name: string }
  /** `templates` are the draft's templates with the old name rewritten (`renameFieldInTemplates`). */
  | { type: "renameField"; key: string; name: string; templates: TemplateDraft[] }
  | { type: "renameTemplate"; key: string; name: string }
  | { type: "templateText"; key: string; side: "front" | "back"; text: string }
  | { type: "css"; css: string }
  | { type: "move"; list: List; key: string; by: -1 | 1 }
  | { type: "sort"; key: string }
  | { type: "remove"; list: List; key: string }
  | { type: "restore"; list: List; key: string }
  | { type: "addField"; name: string }
  | { type: "addTemplate"; name: string };

const fieldItem = (field: FieldDraft, key: string): FieldItem => ({
  key,
  id: field.id,
  name: field.name,
});

const templateItem = (template: TemplateDraft, key: string): TemplateItem => ({
  key,
  id: template.id,
  name: template.name,
  front: template.front,
  back: template.back,
});

/** The state a screen starts from, and returns to after a save. */
export function startState(details: NoteTypeDetails): DraftState {
  const { draft } = details;
  const fields = draft.fields.map((f, i) => fieldItem(f, f.id ?? `n${i}`));
  return {
    name: draft.name,
    css: draft.css,
    sortKey: fields[draft.sortField ?? 0]?.key ?? fields[0]?.key ?? null,
    fields,
    templates: draft.templates.map((t, i) => templateItem(t, t.id ?? `n${i}`)),
    removedFields: details.removedFields.map((f, i) => fieldItem(f, f.id ?? `r${i}`)),
    removedTemplates: details.removedTemplates.map((t, i) => templateItem(t, t.id ?? `r${i}`)),
    next: 0,
  };
}

/** What `checkNoteType` and `saveNoteType` take. */
export function toDraft(state: DraftState): NoteTypeDraft {
  const sort = state.fields.findIndex((f) => f.key === state.sortKey);
  return {
    name: state.name,
    css: state.css,
    sortField: sort === -1 ? null : sort,
    fields: state.fields.map(({ id, name }) => ({ id, name })),
    templates: state.templates.map(({ id, name, front, back }) => ({ id, name, front, back })),
  };
}

export function isChanged(state: DraftState, start: DraftState): boolean {
  return JSON.stringify(toDraft(state)) !== JSON.stringify(toDraft(start));
}

/** Why `name` cannot be used for an item of `items` (other than `except`), or null. */
export function nameProblem(
  items: { key: string; name: string }[],
  name: string,
  except?: string,
): string | null {
  const wanted = name.trim();
  if (!wanted) return "The name is empty. Type a name.";
  const lower = wanted.toLowerCase();
  if (items.some((i) => i.key !== except && i.name.trim().toLowerCase() === lower)) {
    return `There is already one called "${wanted}" in this note type. Choose a different name.`;
  }
  return null;
}

/** The first "Card 2", "Card 3" ... that no card of the note type has. */
export function newCardName(templates: { name: string }[]): string {
  const used = new Set(templates.map((t) => t.name.trim().toLowerCase()));
  for (let n = templates.length + 1; ; n++) {
    if (!used.has(`card ${n}`)) return `Card ${n}`;
  }
}

function moved<T>(items: T[], index: number, by: -1 | 1): T[] {
  const to = index + by;
  if (index < 0 || to < 0 || to >= items.length) return items;
  const copy = items.slice();
  const [item] = copy.splice(index, 1);
  if (item === undefined) return items;
  copy.splice(to, 0, item);
  return copy;
}

export function reducer(state: DraftState, action: Action): DraftState {
  switch (action.type) {
    case "reset":
      return action.state;
    case "name":
      return { ...state, name: action.name };
    case "renameField":
      return {
        ...state,
        fields: state.fields.map((f) =>
          f.key === action.key ? { ...f, name: action.name.trim() } : f,
        ),
        // The screen's own templates are the draft's, in the same order.
        templates: state.templates.map((t, i) => {
          const rewritten = action.templates[i];
          return rewritten ? { ...t, front: rewritten.front, back: rewritten.back } : t;
        }),
      };
    case "renameTemplate":
      return {
        ...state,
        templates: state.templates.map((t) =>
          t.key === action.key ? { ...t, name: action.name.trim() } : t,
        ),
      };
    case "templateText":
      return {
        ...state,
        templates: state.templates.map((t) =>
          t.key === action.key ? { ...t, [action.side]: action.text } : t,
        ),
      };
    case "css":
      return { ...state, css: action.css };
    case "move": {
      const items = state[action.list] as { key: string }[];
      const index = items.findIndex((i) => i.key === action.key);
      return { ...state, [action.list]: moved(items, index, action.by) };
    }
    case "sort":
      return state.fields.some((f) => f.key === action.key)
        ? { ...state, sortKey: action.key }
        : state;
    case "remove": {
      // The last one stays: a note type needs a field and a card.
      if (state[action.list].length <= 1) return state;
      if (action.list === "fields") {
        const gone = state.fields.find((f) => f.key === action.key);
        if (!gone) return state;
        const fields = state.fields.filter((f) => f.key !== action.key);
        return {
          ...state,
          fields,
          sortKey: state.sortKey === gone.key ? (fields[0]?.key ?? null) : state.sortKey,
          // A field that is new and removed again is just gone.
          removedFields: gone.id ? [...state.removedFields, gone] : state.removedFields,
        };
      }
      const gone = state.templates.find((t) => t.key === action.key);
      if (!gone) return state;
      return {
        ...state,
        templates: state.templates.filter((t) => t.key !== action.key),
        removedTemplates: gone.id ? [...state.removedTemplates, gone] : state.removedTemplates,
      };
    }
    case "restore": {
      if (action.list === "fields") {
        const back = state.removedFields.find((f) => f.key === action.key);
        if (!back || nameProblem(state.fields, back.name)) return state;
        return {
          ...state,
          fields: [...state.fields, back],
          removedFields: state.removedFields.filter((f) => f.key !== action.key),
        };
      }
      const back = state.removedTemplates.find((t) => t.key === action.key);
      if (!back || nameProblem(state.templates, back.name)) return state;
      return {
        ...state,
        templates: [...state.templates, back],
        removedTemplates: state.removedTemplates.filter((t) => t.key !== action.key),
      };
    }
    case "addField": {
      const key = `n${state.next}`;
      return {
        ...state,
        fields: [...state.fields, { key, id: null, name: action.name.trim() }],
        next: state.next + 1,
      };
    }
    case "addTemplate": {
      const first = state.templates[0];
      const key = `n${state.next}`;
      return {
        ...state,
        templates: [
          ...state.templates,
          {
            key,
            id: null,
            name: action.name.trim(),
            front: first?.front ?? "",
            back: first?.back ?? "",
          },
        ],
        next: state.next + 1,
      };
    }
  }
}
