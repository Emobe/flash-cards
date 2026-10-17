import { useId, useState } from "react";
import { Dialog } from "../components/Dialog";

/** The largest number of days the core takes (its maximum interval). */
const MAX_DAYS = 36_500;

type Choice = "today" | "tomorrow" | "in" | "between";

/** A whole number of days the core takes, or null. */
function days(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  return n <= MAX_DAYS ? n : null;
}

/** The range a choice stands for, or the reason it is not one yet. */
export function dueRange(
  choice: Choice,
  inDays: string,
  from: string,
  to: string,
): { fromDays: number; toDays: number } | { problem: string } {
  const outside = { problem: "Enter whole days from 0 to 36,500." };
  switch (choice) {
    case "today":
      return { fromDays: 0, toDays: 0 };
    case "tomorrow":
      return { fromDays: 1, toDays: 1 };
    case "in": {
      const n = days(inDays);
      return n === null ? outside : { fromDays: n, toDays: n };
    }
    case "between": {
      const a = days(from);
      const b = days(to);
      if (a === null || b === null) return outside;
      if (a > b) return { problem: "The first number of days cannot be more than the second." };
      return { fromDays: a, toDays: b };
    }
  }
}

/**
 * Set due date (ADR 0014 decision 5): Today, Tomorrow, In N days, or Between N and M days, where
 * the core picks a day in the range for each card. The sheet is the confirmation, since it changes
 * schedules. `note` says what it acts on, and `newCards` how many of those it leaves alone (a new
 * card has nothing to build a review on).
 */
export function DueSheet({
  note,
  newCards,
  busy,
  onApply,
  onClose,
}: {
  note: string;
  newCards: number;
  busy: boolean;
  onApply: (fromDays: number, toDays: number) => void;
  onClose: () => void;
}) {
  const [choice, setChoice] = useState<Choice>("tomorrow");
  const [inDays, setInDays] = useState("7");
  const [from, setFrom] = useState("3");
  const [to, setTo] = useState("7");
  const [problem, setProblem] = useState<string | null>(null);
  const name = useId();
  const problemId = useId();

  const option = (value: Choice, label: React.ReactNode) => (
    <label className="choice">
      <input
        type="radio"
        name={name}
        value={value}
        checked={choice === value}
        onChange={() => {
          setChoice(value);
          setProblem(null);
        }}
      />
      <span className="due-option-text">{label}</span>
    </label>
  );
  const number = (label: string, value: string, set: (text: string) => void, owner: Choice) => (
    <input
      className="due-days"
      inputMode="numeric"
      aria-label={label}
      aria-invalid={problem !== null && choice === owner ? true : undefined}
      aria-describedby={problem !== null && choice === owner ? problemId : undefined}
      value={value}
      onFocus={() => setChoice(owner)}
      onChange={(event) => {
        set(event.target.value);
        setChoice(owner);
        setProblem(null);
      }}
    />
  );

  return (
    <Dialog title="Set due date" variant="sheet" onClose={onClose}>
      <form
        className="due-sheet"
        onSubmit={(event) => {
          event.preventDefault();
          const range = dueRange(choice, inDays, from, to);
          if ("problem" in range) setProblem(range.problem);
          else onApply(range.fromDays, range.toDays);
        }}
      >
        <p className="picker-note">{note}</p>
        <fieldset className="due-choices">
          <legend className="visually-hidden">When</legend>
          {option("today", "Today")}
          {option("tomorrow", "Tomorrow")}
          {option("in", <>In {number("Number of days", inDays, setInDays, "in")} days</>)}
          {option(
            "between",
            <>
              Between {number("Fewest days", from, setFrom, "between")} and{" "}
              {number("Most days", to, setTo, "between")} days
            </>,
          )}
        </fieldset>
        {choice === "between" && (
          <p className="picker-note">
            Each card gets a day in the range, so they do not all come at once.
          </p>
        )}
        {newCards > 0 && (
          <p className="picker-note">
            {newCards === 1 ? "1 new card is" : `${newCards.toLocaleString()} new cards are`} not
            changed: a new card has not been studied yet.
          </p>
        )}
        {problem && (
          <p id={problemId} className="dialog-error" role="alert">
            {problem}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="button button-primary" disabled={busy}>
            Set due date
          </button>
        </div>
      </form>
    </Dialog>
  );
}
