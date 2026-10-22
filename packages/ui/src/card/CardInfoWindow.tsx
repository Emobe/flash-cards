import { type CardInfoOutput, CoreError } from "core-client";
import { useCallback, useEffect, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import {
  describeSchedule,
  entryDetail,
  entryTitle,
  formatMoment,
  formatStability,
} from "./cardInfo";
import "./card-info.css";

/**
 * Everything the app knows about one card (step 3.3): where it is, how it is scheduled, its note
 * and what happened to it. Read only. The note's fields arrive as plain text and are shown as text,
 * never as HTML (CLAUDE.md: card content is untrusted). A full-screen layer on a phone and a wide
 * window from 600 px, over whatever opened it.
 */
export function CardInfoWindow({ cardId, onClose }: { cardId: string; onClose: () => void }) {
  const core = useCore();
  const [info, setInfo] = useState<CardInfoOutput | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  const load = useCallback(() => {
    setFailed(null);
    core
      .call("getCardInfo", { cardId })
      .then(setInfo)
      .catch((failure: unknown) =>
        setFailed(
          failure instanceof CoreError
            ? failure.message
            : "The card information could not be loaded.",
        ),
      );
  }, [core, cardId]);
  useEffect(load, [load]);

  return (
    <Dialog title="Card info" variant="layer" onClose={onClose}>
      <div className="card-info">
        {failed !== null && (
          <div className="problem" role="alert">
            <p>{failed}</p>
            <button type="button" className="button" onClick={load}>
              Try again
            </button>
          </div>
        )}
        {failed === null && info === null && <p role="status">Loading...</p>}
        {info && <Details info={info} />}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </Dialog>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="card-info-row">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function Details({ info }: { info: CardInfoOutput }) {
  const { card, note, history } = info;
  const answers = history.filter((e) => e.kind === "answer");
  return (
    <>
      <section aria-labelledby="card-info-schedule">
        <h3 id="card-info-schedule">Schedule</h3>
        <dl>
          <Row label="Now">{describeSchedule(card)}</Row>
          {card.state !== "new" && card.memory && (
            <>
              <Row label="Stability">{formatStability(card.memory.stability)}</Row>
              <Row label="Difficulty">{`${card.memory.difficulty.toFixed(1)} of 10`}</Row>
            </>
          )}
          {card.daysSinceLastAnswer !== null && (
            <Row label="Last answered">
              {card.daysSinceLastAnswer === 0
                ? "Today"
                : `${card.daysSinceLastAnswer} ${card.daysSinceLastAnswer === 1 ? "day" : "days"} ago`}
            </Row>
          )}
          <Row label="Answers">{card.answers}</Row>
          <Row label="Lapses">{card.lapses}</Row>
          <Row label="Deck">{card.deckName}</Row>
          <Row label="Card">{card.templateName}</Row>
        </dl>
      </section>
      <section aria-labelledby="card-info-note">
        <h3 id="card-info-note">Note</h3>
        <dl>
          <Row label="Note type">{note.noteTypeName}</Row>
          <Row label="Cards">{note.cardCount}</Row>
          <Row label="Tags">{note.tags.length > 0 ? note.tags.join(", ") : "None"}</Row>
          {note.fields.map((field) => (
            <Row key={field.name} label={field.name}>
              <span className="card-info-text">{field.text || "(empty)"}</span>
            </Row>
          ))}
        </dl>
      </section>
      <section aria-labelledby="card-info-history">
        <h3 id="card-info-history">History</h3>
        {history.length === 0 ? (
          <p>Not answered yet.</p>
        ) : (
          <>
            <p className="card-info-summary">
              {answers.length} {answers.length === 1 ? "answer" : "answers"}, newest first.
            </p>
            <ol className="card-info-history" reversed>
              {[...history].reverse().map((entry) => (
                <li key={entry.id} data-kind={entry.kind}>
                  <span className="card-info-when">
                    {formatMoment(entry.timeMs, entry.utcOffset)}
                  </span>
                  <strong>{entryTitle(entry)}</strong>
                  <span className="card-info-detail">{entryDetail(entry)}</span>
                </li>
              ))}
            </ol>
          </>
        )}
      </section>
    </>
  );
}
