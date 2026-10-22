import {
  CoreError,
  type NoteTypeDraft,
  type PreviewNote,
  type TemplateMistakes,
} from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { CardFrame } from "../../card/CardFrame";
import { useCore } from "../../core";
import { clozeNumbers } from "./templateText";

/** How long the draft must stand still before the preview is asked for again (ADR 0015 decision 4). */
export const PREVIEW_DELAY_MS = 400;

/** The most notes the preview steps through. */
const NOTE_LIMIT = 50;

/** A frame that has not reported a height by now is shown anyway. */
const SWAP_AFTER_MS = 1500;

export type Side = "front" | "back";

type Rendered = { front: string; back: string; media: string[]; makesCard: boolean };

export type Preview = {
  /** Null until the notes are listed. */
  notes: PreviewNote[] | null;
  noteIndex: number;
  setNoteIndex: (index: number) => void;
  clozeList: number[];
  ordinal: number;
  setOrdinal: (ordinal: number) => void;
  /** The last render that had no mistake. */
  good: Rendered | null;
  problems: TemplateMistakes[];
  unknownFields: string[];
  failure: string | null;
};

/**
 * Asks the core to render the draft with a real note (`previewCard`), after a pause when the draft
 * changed. Nothing is written. The last good render is kept while the draft has a mistake.
 */
export function usePreview(args: {
  noteTypeId: string;
  draft: NoteTypeDraft;
  templateIndex: number;
  cloze: boolean;
}): Preview {
  const core = useCore();
  const { noteTypeId, draft, templateIndex, cloze } = args;
  const [notes, setNotes] = useState<PreviewNote[] | null>(null);
  const [noteIndex, setNoteIndex] = useState(0);
  const [clozeList, setClozeList] = useState<number[]>([]);
  const [ordinal, setOrdinal] = useState(1);
  const [good, setGood] = useState<Rendered | null>(null);
  const [problems, setProblems] = useState<TemplateMistakes[]>([]);
  const [unknownFields, setUnknownFields] = useState<string[]>([]);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    core.call("previewNotes", { noteTypeId, limit: NOTE_LIMIT }).then(
      (out) => alive && setNotes(out.notes),
      () => alive && setNotes([]),
    );
    return () => {
      alive = false;
    };
  }, [core, noteTypeId]);

  const noteId = notes?.[noteIndex]?.noteId ?? null;

  // A cloze note makes a card per number it uses: list them for the picker.
  useEffect(() => {
    setClozeList([]);
    setOrdinal(1);
    if (!cloze || !noteId) return;
    let alive = true;
    core.call("getNote", { noteId }).then(
      (note) => {
        if (!alive) return;
        const numbers = clozeNumbers(note.fields.map((f) => f.value));
        setClozeList(numbers);
        setOrdinal(numbers[0] ?? 1);
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [core, cloze, noteId]);

  const json = JSON.stringify(draft);
  const lastJson = useRef<string | null>(null);
  const request = useRef(0);
  useEffect(() => {
    if (notes === null) return;
    // Typing waits; choosing another note or number does not.
    const delay = lastJson.current !== null && lastJson.current !== json ? PREVIEW_DELAY_MS : 0;
    lastJson.current = json;
    const mine = ++request.current;
    const timer = setTimeout(() => {
      core
        .call("previewCard", {
          noteTypeId,
          draft: JSON.parse(json) as NoteTypeDraft,
          templateIndex,
          noteId,
          ordinal,
        })
        .then(
          (out) => {
            if (mine !== request.current) return;
            setFailure(null);
            setProblems(out.problems);
            setUnknownFields(out.unknownFields);
            if (out.problems.length === 0 && out.front !== null && out.back !== null) {
              setGood({
                front: out.front,
                back: out.back,
                media: out.media,
                makesCard: out.makesCard,
              });
            }
          },
          (error: unknown) => {
            if (mine !== request.current) return;
            setFailure(
              error instanceof CoreError ? error.message : "The preview could not be made.",
            );
          },
        );
    }, delay);
    return () => clearTimeout(timer);
  }, [core, notes, noteTypeId, json, templateIndex, noteId, ordinal]);

  return {
    notes,
    noteIndex,
    setNoteIndex,
    clozeList,
    ordinal,
    setOrdinal,
    good,
    problems,
    unknownFields,
    failure,
  };
}

type FrameContent = { html: string; media: string[] };

const key = (c: FrameContent) => `${c.html}\0${c.media.join("\0")}`;

/**
 * Two frame slots that take turns. A new render loads in the hidden slot and is shown once it has
 * reported a height, so the old card stays until the new one is ready and the preview does not
 * flash (each `CardFrame` html is a fresh frame, ADR 0005).
 */
function DoubleFrame(props: {
  content: FrameContent;
  theme: "light" | "dark";
  loadMedia: (name: string) => Promise<Blob>;
}) {
  const { content, theme, loadMedia } = props;
  const [slots, setSlots] = useState<[FrameContent | null, FrameContent | null]>([content, null]);
  const [active, setActive] = useState(0);
  /** The slot loading a newer card, shown once it reports a height. */
  const [pending, setPending] = useState<number | null>(null);
  const [height, setHeight] = useState(0);

  // A different card goes into the slot that is not shown.
  const wanted = key(content);
  const wantedNow = useRef(wanted);
  wantedNow.current = wanted;
  useEffect(() => {
    const shown = slots[active];
    if (shown && key(shown) === wanted) {
      // Back to the card on show: nothing is waiting any more.
      if (pending !== null) setPending(null);
      return;
    }
    const waiting = pending === null ? null : slots[pending];
    if (waiting && key(waiting) === wanted) return;
    const target = 1 - active;
    setSlots((now) => {
      const next: [FrameContent | null, FrameContent | null] = [...now];
      next[target] = content;
      return next;
    });
    setPending(target);
  }, [wanted, content, slots, active, pending]);

  const reveal = useCallback((slot: number) => {
    setActive(slot);
    setPending(null);
  }, []);

  // A card that never reports a height is shown anyway.
  useEffect(() => {
    if (pending === null) return;
    const timer = setTimeout(() => reveal(pending), SWAP_AFTER_MS);
    return () => clearTimeout(timer);
  }, [pending, reveal]);

  return (
    <div className="preview-frames" style={{ minHeight: height }}>
      {slots.map((slot, i) =>
        slot ? (
          <div
            // biome-ignore lint/suspicious/noArrayIndexKey: the two slots are fixed positions
            key={i}
            className="preview-frame"
            data-active={i === active}
            aria-hidden={i !== active}
          >
            <CardFrame
              html={slot.html}
              mediaNames={slot.media}
              loadMedia={loadMedia}
              theme={theme}
              onHeight={(px) => {
                if (i === active) setHeight(px);
                // A slot that is waiting is shown once, and only while it is the card wanted.
                else if (i === pending && key(slot) === wantedNow.current) {
                  setHeight(px);
                  reveal(i);
                }
              }}
            />
          </div>
        ) : null,
      )}
    </div>
  );
}

/** The preview pane: which note, which side, and the card. */
export function TemplatePreview(props: {
  preview: Preview;
  side: Side;
  onSide: (side: Side) => void;
  theme: "light" | "dark";
  cloze: boolean;
}) {
  const { preview, side, onSide, theme, cloze } = props;
  const core = useCore();
  const loadMedia = useCallback(
    async (name: string) => {
      const { output, bytes } = await core.call("getMedia", { name });
      return new Blob([new Uint8Array(bytes)], { type: output.contentType });
    },
    [core],
  );
  const { notes, noteIndex, good } = preview;
  const broken = preview.problems.length > 0;
  const none = notes !== null && notes.length === 0;
  const note = notes?.[noteIndex];

  return (
    <section className="template-preview" aria-label="Preview">
      <div className="preview-bar">
        <fieldset className="preview-sides" aria-label="Side">
          {(["front", "back"] as const).map((s) => (
            <button
              key={s}
              type="button"
              className="button"
              aria-pressed={side === s}
              onClick={() => onSide(s)}
            >
              {s === "front" ? "Front" : "Back"}
            </button>
          ))}
        </fieldset>
        {notes !== null && notes.length > 1 && (
          <div className="preview-notes">
            <button
              type="button"
              className="button"
              disabled={noteIndex <= 0}
              onClick={() => preview.setNoteIndex(noteIndex - 1)}
            >
              Previous
            </button>
            <span className="preview-note-label">
              {note?.title || "(empty)"} ({noteIndex + 1} of {notes.length})
            </span>
            <button
              type="button"
              className="button"
              disabled={noteIndex >= notes.length - 1}
              onClick={() => preview.setNoteIndex(noteIndex + 1)}
            >
              Next
            </button>
          </div>
        )}
        {notes !== null && notes.length === 1 && (
          <span className="preview-note-label">{note?.title || "(empty)"}</span>
        )}
        {cloze && preview.clozeList.length > 0 && (
          <label className="preview-cloze">
            Cloze number
            <select
              value={preview.ordinal}
              onChange={(e) => preview.setOrdinal(Number(e.target.value))}
            >
              {preview.clozeList.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      {none && <p className="hint">Example text, because this note type has no notes yet.</p>}
      {good && !good.makesCard && !none && (
        <p className="hint" role="status">
          This note makes no card with this template.
        </p>
      )}
      {broken && (
        <p className="hint" role="status">
          Fix the mistake to update the preview.
        </p>
      )}
      {preview.failure && (
        <p role="alert" className="notetype-error">
          {preview.failure}
        </p>
      )}

      <div className="preview-card" data-stale={broken}>
        {good ? (
          <DoubleFrame
            content={{ html: side === "front" ? good.front : good.back, media: good.media }}
            theme={theme}
            loadMedia={loadMedia}
          />
        ) : (
          !broken && !preview.failure && <p className="hint">Making the preview...</p>
        )}
      </div>
    </section>
  );
}
