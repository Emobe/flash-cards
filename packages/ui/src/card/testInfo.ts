import type { CardInfoOutput } from "core-client";

/** A review card answered twice, then set due by hand, for the card info tests. */
export const infoReview: CardInfoOutput = {
  card: {
    state: "review",
    step: 0,
    due: { kind: "day", days: 3 },
    memory: { stability: 12.34, difficulty: 5.67 },
    daysSinceLastAnswer: 2,
    answers: 2,
    lapses: 0,
    suspended: false,
    buried: false,
    deckId: "d-default",
    deckName: "Polish::Verbs",
    templateName: "Card 1",
  },
  note: {
    noteId: "n-kot",
    noteTypeId: "nt-basic",
    noteTypeName: "Basic",
    fields: [
      { name: "Front", text: "kot <b>not bold</b>" },
      { name: "Back", text: "" },
    ],
    tags: ["animals", "nouns"],
    cardCount: 1,
  },
  history: [
    {
      id: "e1",
      kind: "answer",
      timeMs: Date.UTC(2026, 9, 1, 9, 0),
      utcOffset: 0,
      rating: "good",
      stateBefore: "new",
      state: "learning",
      daysSincePrevious: null,
      due: { kind: "minutes", minutes: 10 },
      durationMs: 4_000,
      memory: null,
    },
    {
      id: "e2",
      kind: "answer",
      timeMs: Date.UTC(2026, 9, 1, 9, 10),
      utcOffset: 0,
      rating: "easy",
      stateBefore: "learning",
      state: "review",
      daysSincePrevious: 0,
      due: { kind: "days", days: 4 },
      durationMs: 2_000,
      memory: { stability: 4, difficulty: 5 },
    },
    {
      id: "e3",
      kind: "setDue",
      timeMs: Date.UTC(2026, 9, 3, 12, 0),
      utcOffset: 0,
      rating: null,
      stateBefore: "review",
      state: "review",
      daysSincePrevious: null,
      due: { kind: "days", days: 5 },
      durationMs: 0,
      memory: null,
    },
  ],
};

export const infoNew: CardInfoOutput = {
  card: {
    ...infoReview.card,
    state: "new",
    due: null,
    memory: null,
    daysSinceLastAnswer: null,
    answers: 0,
  },
  note: infoReview.note,
  history: [],
};
