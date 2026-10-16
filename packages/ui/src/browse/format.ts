import type { SearchResultRow } from "core-client";

/**
 * The state or due line of a result row (ADR 0013 decision 3). Suspended and buried come first,
 * because they say why a due card is not being studied.
 */
export function formatDue(row: Pick<SearchResultRow, "state" | "due" | "suspended" | "buried">) {
  if (row.suspended) return "Suspended";
  if (row.buried) return "Buried until tomorrow";
  if (row.state === "new") return "New";
  const due = row.due;
  if (due === null) return row.state === "review" ? "Review" : "Learning";
  if (due.kind === "time") {
    if (due.minutes <= 0) return "Learning";
    if (due.minutes < 60) return `In ${plural(due.minutes, "minute")}`;
    return `In ${plural(Math.round(due.minutes / 60), "hour")}`;
  }
  if (due.days === 0) return "Due today";
  if (due.days === 1) return "Due tomorrow";
  if (due.days > 1) return `Due in ${plural(due.days, "day")}`;
  return `${plural(-due.days, "day")} overdue`;
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/** A field's plain text as a row shows it: a sound becomes ♪. */
export function rowText(text: string): string {
  return text.replace(/\[sound:[^\]]*\]/g, "♪");
}
