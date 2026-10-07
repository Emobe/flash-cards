import type { IntervalPreview } from "core-client";

const MINUTES_PER_DAY = 1440;
const DAYS_PER_MONTH = 30.4;
const DAYS_PER_YEAR = 365;

/** One decimal, and none when it would be ".0". */
function short(value: number): string {
  const rounded = Math.round(value * 10) / 10;
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

/**
 * How long until a card comes back, as the answer buttons show it: "<1m", "10m", "5h", "3d",
 * "2.5mo", "1.2y". The core gives minutes (learning steps) or days (reviews).
 */
export function formatInterval(preview: IntervalPreview): string {
  const days = preview.unit === "minutes" ? preview.amount / MINUTES_PER_DAY : preview.amount;
  if (preview.unit === "minutes" && days < 1) {
    if (preview.amount < 1) return "<1m";
    if (preview.amount < 60) return `${preview.amount}m`;
    return `${Math.round(preview.amount / 60)}h`;
  }
  if (days < 1) return "<1d";
  if (days < 30) return `${Math.round(days)}d`;
  if (days < DAYS_PER_YEAR) return `${short(days / DAYS_PER_MONTH)}mo`;
  return `${short(days / DAYS_PER_YEAR)}y`;
}

function plural(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? "" : "s"}`;
}

/** A length of time in plain words: "40 seconds", "5 minutes", "1 hour 5 minutes". */
export function formatSpan(ms: number): string {
  const seconds = Math.max(Math.round(ms / 1000), 0);
  if (seconds < 60) return plural(seconds, "second");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return plural(minutes, "minute");
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? plural(hours, "hour") : `${plural(hours, "hour")} ${plural(rest, "minute")}`;
}
