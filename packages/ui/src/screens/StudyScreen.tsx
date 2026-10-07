import type { EventRating, RenderedCardOutput, StudyCounts } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { CardFrame, type CardFrameHandle } from "../card/CardFrame";
import { useCore } from "../core";
import { Link, PageHeading, useRouter } from "../router";
import { WarningIcon } from "../shell/icons";
import { BottomAction } from "../shell/slot";
import { isTypingTarget } from "../shell/useShortcuts";
import { formatInterval, formatSpan } from "../study/format";
import { type Side, type StudyCard, type StudyView, useStudy } from "../study/useStudy";
import { useTheme } from "../theme";
import "../study/study.css";

const RATINGS: { rating: EventRating; label: string; key: string }[] = [
  { rating: "again", label: "Again", key: "1" },
  { rating: "hard", label: "Hard", key: "2" },
  { rating: "good", label: "Good", key: "3" },
  { rating: "easy", label: "Easy", key: "4" },
];

/**
 * The review screen (step 2.3): a card, Show answer, then four answers that each say when the card
 * comes back. Full screen, with the shell's back control. The card is shown only through
 * `CardFrame` (ADR 0005); this screen never touches card HTML.
 */
export function StudyScreen({ deckId }: { deckId: string }) {
  const study = useStudy(deckId);
  const { view } = study;
  // The card frame starts at height 0, so keep the last height until the next card reports its own.
  const [lastHeight, setLastHeight] = useState(0);

  return (
    <div className="study">
      <PageHeading className="visually-hidden">Study</PageHeading>
      {view.kind === "loading" && <p role="status">Opening the deck...</p>}
      {view.kind === "failed" && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{view.error.message}</p>
          <button type="button" className="button" onClick={() => void study.start()}>
            Try again
          </button>
          <Link path="/decks">Back to decks</Link>
        </div>
      )}
      {view.kind === "card" && (
        <CardView
          key={`${view.card.cardId}:${view.side}`}
          card={view.card}
          html={view.html}
          side={view.side}
          pending={study.pending}
          canUndo={study.canUndo}
          minHeight={lastHeight}
          onHeight={setLastHeight}
          onShow={study.show}
          onRate={(rating) => void study.rate(rating)}
          onUndo={() => void study.undo()}
        />
      )}
      {view.kind === "finished" && (
        <Finished
          view={view}
          pending={study.pending}
          canUndo={study.canUndo}
          onAgain={() => void study.start()}
          onUndo={() => void study.undo()}
        />
      )}
    </div>
  );
}

function Counts({ counts, active }: { counts: StudyCounts; active: StudyCard["state"] }) {
  const current = active === "relearning" ? "learning" : active;
  return (
    <ul className="study-counts" aria-label="Left to study today">
      <li data-active={current === "new"}>New {counts.newCount}</li>
      <li data-active={current === "learning"}>Learn {counts.learningCount}</li>
      <li data-active={current === "review"}>Review {counts.reviewCount}</li>
    </ul>
  );
}

function CardView({
  card,
  html,
  side,
  pending,
  canUndo,
  minHeight,
  onHeight,
  onShow,
  onRate,
  onUndo,
}: {
  card: StudyCard;
  html: RenderedCardOutput;
  side: Side;
  pending: boolean;
  canUndo: boolean;
  minHeight: number;
  onHeight: (px: number) => void;
  onShow: () => void;
  onRate: (rating: EventRating) => void;
  onUndo: () => void;
}) {
  const core = useCore();
  const { effective } = useTheme();
  const frame = useRef<CardFrameHandle>(null);
  const [audioCount, setAudioCount] = useState(0);
  const [blocked, setBlocked] = useState(false);

  const loadMedia = useCallback(
    async (name: string) => {
      const { output, bytes } = await core.call("getMedia", { name });
      // A copy, because the reply may be a view on a larger buffer.
      return new Blob([new Uint8Array(bytes)], { type: output.contentType });
    },
    [core],
  );

  // Keys on the desktop. Keys pressed inside the card go to the card, never here (ADR 0005), and
  // the buttons always work, so a click in the card only costs the shortcuts until focus leaves it.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.altKey || event.metaKey || event.isComposing) return;
      if (isTypingTarget(event.target)) return;
      const key = event.key.toLowerCase();
      if (event.ctrlKey) {
        if (key === "z" && canUndo && !pending) {
          event.preventDefault();
          onUndo();
        }
        return;
      }
      // On a focused button, Enter and Space already press it.
      const onControl = event.target instanceof Element && event.target.closest("button, a");
      if (key === " " || key === "enter") {
        if (onControl) return;
        event.preventDefault();
        if (side === "question") onShow();
        else onRate("good");
      } else if (side === "answer" && ["1", "2", "3", "4"].includes(key)) {
        onRate(RATINGS[Number(key) - 1]?.rating ?? "good");
      } else if (key === "z" && canUndo && !pending) {
        onUndo();
      } else if (key === "r" && audioCount > 0) {
        frame.current?.play();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [side, canUndo, pending, audioCount, onShow, onRate, onUndo]);

  return (
    <>
      <div className="study-top">
        <Counts counts={card.counts} active={card.state} />
        <button
          type="button"
          className="button study-undo"
          disabled={!canUndo || pending}
          onClick={onUndo}
        >
          Undo <kbd>Z</kbd>
        </button>
      </div>
      <div className="study-card" style={{ minHeight }}>
        <CardFrame
          ref={frame}
          html={side === "question" ? html.front : html.back}
          mediaNames={html.media}
          loadMedia={loadMedia}
          theme={effective}
          autoplay
          onHeight={onHeight}
          onAudio={setAudioCount}
          onAutoplayBlocked={() => setBlocked(true)}
        />
      </div>
      {audioCount > 0 && (
        <div className="study-sound">
          <button type="button" className="button" onClick={() => frame.current?.play()}>
            Replay sound <kbd>R</kbd>
          </button>
          {blocked && (
            <p role="status">The sound did not start by itself. Press Replay sound to hear it.</p>
          )}
        </div>
      )}
      <p role="status" className="visually-hidden">
        {side === "question" ? "Question" : "Answer shown"}
      </p>
      <BottomAction>
        {side === "question" ? (
          <button
            type="button"
            className="button button-primary study-show"
            disabled={pending}
            onClick={onShow}
          >
            Show answer <kbd>Space</kbd>
          </button>
        ) : (
          <fieldset className="study-ratings">
            <legend className="visually-hidden">How well did you remember it?</legend>
            {RATINGS.map(({ rating, label, key }, index) => (
              <button
                key={rating}
                type="button"
                className={`button study-rating study-rating-${rating}${rating === "good" ? " button-primary" : ""}`}
                disabled={pending}
                onClick={() => onRate(rating)}
              >
                <span className="study-rating-name">{label}</span>
                <span className="study-rating-interval">
                  {card.previews[index] ? formatInterval(card.previews[index]) : ""}
                </span>
                <kbd>{key}</kbd>
              </button>
            ))}
          </fieldset>
        )}
      </BottomAction>
    </>
  );
}

function Finished({
  view,
  pending,
  canUndo,
  onAgain,
  onUndo,
}: {
  view: Extract<StudyView, { kind: "finished" }>;
  pending: boolean;
  canUndo: boolean;
  onAgain: () => void;
  onUndo: () => void;
}) {
  const { navigate, back } = useRouter();
  const { summary, waitSeconds } = view;
  // The deck list is where study was opened from. A reload of this route has nothing behind it.
  const toDecks = () => (window.history.length > 1 ? back() : navigate("/decks"));
  const answered = summary?.answered ?? 0;
  return (
    <>
      <section className="study-done" aria-labelledby="study-done-title">
        <h2 id="study-done-title">
          {waitSeconds !== null ? "Nothing to show right now" : "Done for now"}
        </h2>
        {summary && answered > 0 ? (
          <ul className="study-summary">
            <li>
              You answered {answered} {answered === 1 ? "card" : "cards"}.
            </li>
            {summary.again > 0 && <li>{summary.again} to see again soon.</li>}
            <li>Time studied: {formatSpan(summary.studiedMs)}.</li>
          </ul>
        ) : (
          <p>No cards were answered this time.</p>
        )}
        {waitSeconds !== null ? (
          <p role="status">The next card is due in {formatSpan(waitSeconds * 1000)}.</p>
        ) : (
          <p>Nothing else is due today.</p>
        )}
        <div className="study-done-actions">
          {waitSeconds !== null && (
            <button type="button" className="button" disabled={pending} onClick={onAgain}>
              Check again
            </button>
          )}
          {canUndo && (
            <button type="button" className="button" disabled={pending} onClick={onUndo}>
              Undo last answer
            </button>
          )}
        </div>
      </section>
      <BottomAction>
        <button type="button" className="button button-primary study-show" onClick={toDecks}>
          Back to decks
        </button>
      </BottomAction>
    </>
  );
}
