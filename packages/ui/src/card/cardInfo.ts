import type { CardInfoCard, CardInfoDue, CardInfoEntry } from "core-client";
import { formatDue } from "../browse/format";

/** The state, or why the card is not being studied, and when it is due: `Review, due in 3 days`. */
export function describeSchedule(
  card: Pick<CardInfoCard, "state" | "due" | "suspended" | "buried">,
) {
  const state = card.state === "relearning" ? "Relearning" : capital(card.state);
  const when = formatDue(card);
  if (when === state) return state;
  return `${state}: ${when.charAt(0).toLowerCase()}${when.slice(1)}`;
}

function capital(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * When something happened, in the local time of the device it happened on (`utcOffset` minutes),
 * so a history reads the same wherever it is opened.
 */
export function formatMoment(timeMs: number, utcOffset: number): string {
  return new Date(timeMs + utcOffset * 60_000).toLocaleString(undefined, {
    timeZone: "UTC",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/** `10 minutes`, `2 days`: how long after an entry the card came back. */
export function formatBackIn(due: CardInfoDue): string {
  if (due.kind === "days") return due.days === 0 ? "today" : plural(due.days, "day");
  if (due.minutes >= 60) return plural(Math.round(due.minutes / 60), "hour");
  return plural(due.minutes, "minute");
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return "under 1 second";
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return plural(seconds, "second");
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

/** What stability means to a person: days until the chance of remembering falls to 90 %. */
export function formatStability(days: number): string {
  if (days < 1) return "under 1 day";
  return plural(Math.round(days * 10) / 10, "day");
}

const RATINGS = { again: "Again", hard: "Hard", good: "Good", easy: "Easy" } as const;

/** The title of an entry: the rating of an answer, or what was done to the card. */
export function entryTitle(entry: CardInfoEntry): string {
  switch (entry.kind) {
    case "answer":
      return entry.rating ? RATINGS[entry.rating] : "Answer";
    case "setDue":
      return "Due date set by hand";
    case "reset":
      return "Reset to new";
    case "transfer":
      return "Took the schedule of the card it replaced";
  }
}

/** The line under an entry's title: what it did to the card. */
export function entryDetail(entry: CardInfoEntry): string {
  const parts: string[] = [];
  if (entry.kind === "answer" && entry.daysSincePrevious !== null) {
    parts.push(`${plural(entry.daysSincePrevious, "day")} after the last answer`);
  }
  if (entry.stateBefore && entry.state && entry.kind !== "transfer") {
    parts.push(
      entry.stateBefore === entry.state
        ? capital(entry.state)
        : `${capital(entry.stateBefore)} to ${entry.state}`,
    );
  }
  if (entry.due && entry.kind !== "reset") parts.push(`back in ${formatBackIn(entry.due)}`);
  if (entry.kind === "answer" && entry.durationMs > 0) {
    parts.push(`took ${formatDuration(entry.durationMs)}`);
  }
  return parts.join(" · ");
}
