import {
  CoreError,
  type EventRating,
  type RenderedCardOutput,
  type SessionSummary,
  type StudyNext,
} from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "../core";

export type StudyCard = Extract<StudyNext, { kind: "card" }>;
export type Side = "question" | "answer";

export type StudyView =
  | { kind: "loading" }
  | { kind: "failed"; error: CoreError }
  | {
      kind: "card";
      card: StudyCard;
      html: RenderedCardOutput;
      side: Side;
      /** `performance.now()` when the question appeared, for the time the answer took. */
      shownAt: number;
    }
  | {
      kind: "finished";
      /** What the session did, or null if the core no longer had it open. */
      summary: SessionSummary | null;
      /** Whether a learning card is due later today, and in how many seconds. */
      waitSeconds: number | null;
    };

function asCoreError(error: unknown): CoreError {
  return error instanceof CoreError
    ? error
    : new CoreError("internal", "Something went wrong while studying. Try again.");
}

/**
 * One study session on a deck (ADR 0009): it starts when the screen opens and ends when it closes
 * or the cards run out. Leaving in the middle of a card loses nothing, because an answer is
 * recorded only when it is rated. The card stays due.
 */
export function useStudy(deckId: string) {
  const core = useCore();
  const [view, setView] = useState<StudyView>({ kind: "loading" });
  const [pending, setPending] = useState(false);
  /** Answers given on this screen and not undone, so Undo is offered only when it has something. */
  const [answered, setAnswered] = useState(0);
  const session = useRef<string | null>(null);
  const busy = useRef(false);
  const alive = useRef(true);
  const viewRef = useRef(view);
  viewRef.current = view;

  const endSession = useCallback(async (): Promise<SessionSummary | null> => {
    const sessionId = session.current;
    session.current = null;
    if (!sessionId) return null;
    return (await core.call("endStudySession", { sessionId })).summary;
  }, [core]);

  const startSession = useCallback(async () => {
    const { sessionId } = await core.call("startStudySession", { deckId });
    session.current = sessionId;
  }, [core, deckId]);

  const loadNext = useCallback(async () => {
    const next = await core.call("nextCard", { deckId });
    if (!alive.current) return;
    if (next.kind === "card") {
      const html = await core.call("renderCard", { cardId: next.cardId });
      if (!alive.current) return;
      setView({ kind: "card", card: next, html, side: "question", shownAt: performance.now() });
      return;
    }
    const summary = await endSession();
    if (!alive.current) return;
    setView({
      kind: "finished",
      summary,
      waitSeconds: next.kind === "waiting" ? next.waitSeconds : null,
    });
  }, [core, deckId, endSession]);

  /** Runs one action at a time. A second tap or key while one runs does nothing. */
  const exclusive = useCallback(async (action: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    try {
      await action();
    } catch (error) {
      if (alive.current) setView({ kind: "failed", error: asCoreError(error) });
    } finally {
      busy.current = false;
      if (alive.current) setPending(false);
    }
  }, []);

  const start = useCallback(
    () =>
      exclusive(async () => {
        setView({ kind: "loading" });
        await startSession();
        // The screen closed while the session was starting.
        if (!alive.current) {
          await endSession();
          return;
        }
        await loadNext();
      }),
    [exclusive, startSession, endSession, loadNext],
  );

  useEffect(() => {
    alive.current = true;
    void start();
    return () => {
      alive.current = false;
      endSession().catch(() => {
        // The session only exists in memory, so there is nothing to clean up if this fails.
      });
    };
  }, [start, endSession]);

  const show = useCallback(() => {
    const current = viewRef.current;
    if (current.kind === "card" && current.side === "question" && !busy.current) {
      setView({ ...current, side: "answer" });
    }
  }, []);

  const rate = useCallback(
    (rating: EventRating) =>
      exclusive(async () => {
        const current = viewRef.current;
        if (current.kind !== "card" || current.side !== "answer") return;
        await core.call("answerCard", {
          cardId: current.card.cardId,
          rating,
          durationMs: Math.max(Math.round(performance.now() - current.shownAt), 0),
        });
        setAnswered((n) => n + 1);
        await loadNext();
      }),
    [core, exclusive, loadNext],
  );

  const undo = useCallback(
    () =>
      exclusive(async () => {
        const { undone } = await core.call("undoAnswer", null);
        if (!undone) return;
        setAnswered((n) => Math.max(n - 1, 0));
        // From the end screen the session is over, so the card comes back in a new one.
        if (session.current === null) await startSession();
        await loadNext();
      }),
    [core, exclusive, startSession, loadNext],
  );

  return { view, pending, canUndo: answered > 0, start, show, rate, undo };
}
