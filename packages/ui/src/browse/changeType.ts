import type { NoteTypeSummary } from "core-client";

/**
 * What fills each field of the new note type, and which old card gives each new template its
 * schedule (ADR 0014 decision 6). An empty string means nothing: the field is left empty, the
 * card starts as new.
 */
export type Maps = {
  /** New field ID to old field ID. */
  fields: Record<string, string>;
  /** New template ID to old template ID. */
  templates: Record<string, string>;
};

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/**
 * Pairs each new part with an old one: the one with the same name ignoring case, then the one in
 * the same position. An old part is used once, so a default never copies a field or gives one
 * card's schedule to two.
 */
function pair(
  from: { id: string; name: string }[],
  to: { id: string; name: string }[],
): Record<string, string> {
  const out: Record<string, string> = {};
  const used = new Set<string>();
  for (const part of to) {
    const match = from.find((old) => !used.has(old.id) && same(old.name, part.name));
    if (match) {
      out[part.id] = match.id;
      used.add(match.id);
    }
  }
  to.forEach((part, index) => {
    if (out[part.id] !== undefined) return;
    const old = from[index];
    if (old && !used.has(old.id)) {
      out[part.id] = old.id;
      used.add(old.id);
    }
  });
  for (const part of to) out[part.id] ??= "";
  return out;
}

/** The maps the window starts with. */
export function defaultMaps(from: NoteTypeSummary, to: NoteTypeSummary): Maps {
  return { fields: pair(from.fields, to.fields), templates: pair(from.templates, to.templates) };
}

/** Whether cards can keep their schedules: both note types are standard or both are cloze. */
export function carries(from: NoteTypeSummary, to: NoteTypeSummary): boolean {
  return from.kind === to.kind;
}

function list(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * What the change will do, as sentences for the window to show before Change: how many notes,
 * what happens to each new card and which fields are not used.
 */
export function describeChange(
  notes: number,
  from: NoteTypeSummary,
  to: NoteTypeSummary,
  maps: Maps,
): string {
  const parts = [`${notes.toLocaleString()} ${notes === 1 ? "note" : "notes"}.`];
  if (!carries(from, to)) {
    parts.push("Every card starts as new, since one note type is cloze and the other is not.");
  } else if (to.kind === "cloze") {
    parts.push("Cards keep their schedule by cloze number.");
  } else {
    for (const template of to.templates) {
      const old = from.templates.find((t) => t.id === maps.templates[template.id]);
      if (!old) parts.push(`${template.name} starts as new.`);
      else if (same(old.name, template.name)) parts.push(`${template.name} keeps its schedule.`);
      else parts.push(`${template.name} takes the schedule of ${old.name}.`);
    }
  }
  const unused = from.fields.filter((f) => !Object.values(maps.fields).includes(f.id));
  if (unused.length > 0) {
    parts.push(
      `${list(unused.map((f) => f.name))} ${unused.length === 1 ? "is" : "are"} not used.`,
    );
  }
  const empty = to.fields.filter((f) => !maps.fields[f.id]);
  if (empty.length > 0) {
    parts.push(
      `${list(empty.map((f) => f.name))} ${empty.length === 1 ? "is" : "are"} left empty.`,
    );
  }
  return parts.join(" ");
}
