import type { DeckSummary, PresetSummary } from "core-client";
import { CoreError } from "core-client";
import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { PageHeading, useRouter } from "../router";
import { BackIcon, WarningIcon } from "../shell/icons";

/** What each setting means, in plain words. Shown under the field, always, so it works on touch. */
export const PRESET_HELP = {
  newPerDay:
    "The most new cards shown in a day. A higher number means you learn faster, but you will have more reviews to do in the days after.",
  reviewsPerDay:
    "The most cards you have already seen that are shown in a day. Cards over the limit wait until tomorrow.",
  learningSteps:
    "How soon a new card comes back while you are still learning it, in minutes. “1 10” shows it again after 1 minute, then after 10 more, and then it becomes a normal review card. Separate the steps with spaces.",
  relearningSteps:
    "The same for a card you forgot. Leave it empty to send a forgotten card straight back to normal reviews with a shorter wait.",
  desiredRetention:
    "How likely you want to be to remember a card on the day it comes up. Higher means fewer forgotten cards but more reviews every day. 90% is a good start.",
  spaceSiblings:
    "When two cards come from the same note, hold the second one until tomorrow, so answering one does not give away the other.",
} as const;

/** `1 10` or `1, 10` as minutes, or the sentence to show when it is not right. */
export function parseSteps(text: string): number[] | string {
  const parts = text.split(/[\s,]+/).filter(Boolean);
  if (parts.length > 8) return "Use at most 8 steps.";
  const steps: number[] = [];
  for (const part of parts) {
    if (!/^\d+$/.test(part)) return "Use whole numbers of minutes, like 1 10.";
    const minutes = Number(part);
    if (minutes < 1 || minutes > 1440) return "Each step is from 1 to 1440 minutes.";
    steps.push(minutes);
  }
  return steps;
}

function parseLimit(text: string): number | string {
  if (!/^\d+$/.test(text.trim())) return "Use a whole number, like 20.";
  const limit = Number(text);
  return limit > 9999 ? "A daily limit can be at most 9999." : limit;
}

type Loaded =
  | { status: "loading" }
  | { status: "ready"; deck: DeckSummary; presets: PresetSummary[] }
  | { status: "failed"; error: CoreError };

type Dialogue = { kind: "create" | "rename" | "delete" } | null;

function asCoreError(error: unknown, fallback: string): CoreError {
  return error instanceof CoreError ? error : new CoreError("internal", fallback);
}

/**
 * The options of one deck (step 2.5b): which preset it uses, and the settings of that preset, each
 * with a plain explanation. A preset is shared, so a change here changes every deck that uses it.
 */
export function OptionsScreen({ deckId }: { deckId: string }) {
  const core = useCore();
  const { navigate, back } = useRouter();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [dialogue, setDialogue] = useState<Dialogue>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const selectId = useId();

  const load = useCallback(() => {
    Promise.all([core.call("getDeckList", null), core.call("getPresets", null)])
      .then(([list, presets]) => {
        const deck = list.decks.find((d) => d.id === deckId);
        setLoaded(
          deck
            ? { status: "ready", deck, presets: presets.presets }
            : {
                status: "failed",
                error: new CoreError("notFound", "That deck was deleted. Go back to the decks."),
              },
        );
      })
      .catch((error: unknown) =>
        setLoaded({
          status: "failed",
          error: asCoreError(error, "Something went wrong while reading the options."),
        }),
      );
  }, [core, deckId]);

  useEffect(load, [load]);

  const toDecks = () => (window.history.length > 1 ? back() : navigate("/decks"));

  async function choose(presetId: string) {
    setProblem(null);
    setNotice(null);
    try {
      await core.call("setDeckPreset", { deckId, presetId });
      load();
    } catch (e) {
      setProblem(asCoreError(e, "Could not choose that preset.").message);
    }
  }

  return (
    <>
      <button type="button" className="button back-link" onClick={toDecks}>
        <BackIcon />
        Decks
      </button>
      {loaded.status === "loading" && <p role="status">Loading the options...</p>}
      {loaded.status === "failed" && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{loaded.error.message}</p>
          <button type="button" className="button" onClick={toDecks}>
            Back to decks
          </button>
        </div>
      )}
      {loaded.status === "ready" &&
        (() => {
          const { deck, presets } = loaded;
          const preset = presets.find((p) => p.id === deck.presetId) ?? presets[0];
          if (!preset) return null;
          return (
            <>
              <PageHeading>Options: {deck.name}</PageHeading>
              <section className="options-section" aria-labelledby={`${selectId}-h`}>
                <h2 id={`${selectId}-h`}>Options preset</h2>
                <p className="options-help">
                  A preset is a set of options that decks share. Changing it changes every deck that
                  uses it.
                </p>
                <label htmlFor={selectId}>This deck uses</label>
                <select id={selectId} value={preset.id} onChange={(e) => choose(e.target.value)}>
                  {presets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.deckCount} {p.deckCount === 1 ? "deck" : "decks"})
                    </option>
                  ))}
                </select>
                <div className="options-buttons">
                  <button
                    type="button"
                    className="button"
                    onClick={() => setDialogue({ kind: "create" })}
                  >
                    New preset
                  </button>
                  <button
                    type="button"
                    className="button"
                    onClick={() => setDialogue({ kind: "rename" })}
                  >
                    Rename
                  </button>
                  {!preset.isDefault && (
                    <button
                      type="button"
                      className="button button-danger-text"
                      onClick={() => setDialogue({ kind: "delete" })}
                    >
                      Delete
                    </button>
                  )}
                </div>
                {problem && (
                  <p role="alert" className="dialog-error">
                    {problem}
                  </p>
                )}
                {notice && <p role="status">{notice}</p>}
              </section>
              <PresetForm
                key={preset.id}
                preset={preset}
                onSaved={() => {
                  setNotice(null);
                  load();
                }}
              />
              {dialogue && (
                <PresetDialog
                  kind={dialogue.kind}
                  preset={preset}
                  onClose={() => setDialogue(null)}
                  onSubmit={async (name) => {
                    if (dialogue.kind === "create") {
                      const made = await core.call("createPreset", { name });
                      await core.call("setDeckPreset", { deckId, presetId: made.id });
                    } else if (dialogue.kind === "rename") {
                      await core.call("renamePreset", { presetId: preset.id, name });
                    } else {
                      const gone = await core.call("deletePreset", { presetId: preset.id });
                      setNotice(
                        `Deleted "${preset.name}". ${gone.decks === 1 ? "1 deck uses" : `${gone.decks} decks use`} the Default preset now.`,
                      );
                    }
                    setDialogue(null);
                    load();
                  }}
                />
              )}
            </>
          );
        })()}
    </>
  );
}

function PresetDialog({
  kind,
  preset,
  onSubmit,
  onClose,
}: {
  kind: "create" | "rename" | "delete";
  preset: PresetSummary;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}) {
  const nameId = useId();
  const [name, setName] = useState(kind === "rename" ? preset.name : "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onSubmit(name);
    } catch (e) {
      setError(asCoreError(e, "Something went wrong. Close this and try again.").message);
      setBusy(false);
    }
  }

  const title = { create: "New preset", rename: "Rename preset", delete: "Delete preset" }[kind];
  return (
    <Dialog title={title} onClose={onClose}>
      <form onSubmit={submit} className="dialog-form">
        {kind === "delete" ? (
          <p>
            Delete <strong>{preset.name}</strong>?{" "}
            {preset.deckCount > 0
              ? `The ${preset.deckCount === 1 ? "deck that uses" : `${preset.deckCount} decks that use`} it will use the Default preset instead.`
              : "No deck uses it."}
          </p>
        ) : (
          <div className="dialog-field">
            <label htmlFor={nameId}>Name</label>
            <input
              id={nameId}
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="off"
            />
            {kind === "create" && (
              <p className="options-help">
                It starts with the usual options, and this deck will use it.
              </p>
            )}
          </div>
        )}
        {error && (
          <p role="alert" className="dialog-error">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className={kind === "delete" ? "button button-danger" : "button button-primary"}
            disabled={busy}
          >
            {{ create: "Create", rename: "Rename", delete: "Delete" }[kind]}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function PresetForm({ preset, onSaved }: { preset: PresetSummary; onSaved: () => void }) {
  const core = useCore();
  const [newPerDay, setNewPerDay] = useState(String(preset.newPerDay));
  const [reviewsPerDay, setReviewsPerDay] = useState(String(preset.reviewsPerDay));
  const [learning, setLearning] = useState(preset.learningSteps.join(" "));
  const [relearning, setRelearning] = useState(preset.relearningSteps.join(" "));
  const [retention, setRetention] = useState(Math.round(preset.desiredRetention * 100));
  const [siblings, setSiblings] = useState(preset.spaceSiblings);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [problem, setProblem] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  async function save(event: FormEvent) {
    event.preventDefault();
    setSaved(false);
    setProblem(null);
    const found: Record<string, string> = {};
    const fields = {
      newPerDay: parseLimit(newPerDay),
      reviewsPerDay: parseLimit(reviewsPerDay),
      learningSteps: parseSteps(learning),
      relearningSteps: parseSteps(relearning),
    };
    for (const [key, value] of Object.entries(fields)) {
      if (typeof value === "string") found[key] = value;
    }
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    setBusy(true);
    try {
      await core.call("setPresetOptions", {
        presetId: preset.id,
        newPerDay: fields.newPerDay as number,
        reviewsPerDay: fields.reviewsPerDay as number,
        learningSteps: fields.learningSteps as number[],
        relearningSteps: fields.relearningSteps as number[],
        desiredRetention: retention / 100,
        spaceSiblings: siblings,
      });
      setSaved(true);
      onSaved();
    } catch (e) {
      setProblem(asCoreError(e, "Could not save the options.").message);
    }
    setBusy(false);
  }

  function edit<T>(set: (value: T) => void, value: T) {
    set(value);
    setSaved(false);
  }
  return (
    <form className="options-form" onSubmit={save} noValidate>
      <h2>Settings in "{preset.name}"</h2>
      {preset.deckCount > 1 && (
        <p className="options-help">These apply to all {preset.deckCount} decks that use it.</p>
      )}
      <Field
        name="newPerDay"
        label="New cards per day"
        help={PRESET_HELP.newPerDay}
        error={errors.newPerDay}
      >
        {(props) => (
          <input
            {...props}
            inputMode="numeric"
            value={newPerDay}
            onChange={(e) => edit(setNewPerDay, e.target.value)}
          />
        )}
      </Field>
      <Field
        name="reviewsPerDay"
        label="Maximum reviews per day"
        help={PRESET_HELP.reviewsPerDay}
        error={errors.reviewsPerDay}
      >
        {(props) => (
          <input
            {...props}
            inputMode="numeric"
            value={reviewsPerDay}
            onChange={(e) => edit(setReviewsPerDay, e.target.value)}
          />
        )}
      </Field>
      <Field
        name="learningSteps"
        label="Learning steps (minutes)"
        help={PRESET_HELP.learningSteps}
        error={errors.learningSteps}
      >
        {(props) => (
          <input
            {...props}
            autoComplete="off"
            value={learning}
            onChange={(e) => edit(setLearning, e.target.value)}
          />
        )}
      </Field>
      <Field
        name="relearningSteps"
        label="Relearning steps (minutes)"
        help={PRESET_HELP.relearningSteps}
        error={errors.relearningSteps}
      >
        {(props) => (
          <input
            {...props}
            autoComplete="off"
            value={relearning}
            onChange={(e) => edit(setRelearning, e.target.value)}
          />
        )}
      </Field>
      <Field
        name="desiredRetention"
        label={`Desired retention: ${retention}%`}
        help={PRESET_HELP.desiredRetention}
      >
        {(props) => (
          <input
            {...props}
            type="range"
            min={70}
            max={99}
            step={1}
            value={retention}
            onChange={(e) => edit(setRetention, Number(e.target.value))}
          />
        )}
      </Field>
      <Field
        name="spaceSiblings"
        label="Delay sibling cards"
        help={PRESET_HELP.spaceSiblings}
        check
      >
        {(props) => (
          <input
            {...props}
            type="checkbox"
            checked={siblings}
            onChange={(e) => edit(setSiblings, e.target.checked)}
          />
        )}
      </Field>
      {problem && (
        <p role="alert" className="dialog-error">
          {problem}
        </p>
      )}
      <div className="options-save">
        <button type="submit" className="button button-primary" disabled={busy}>
          Save
        </button>
        {saved && <span role="status">Saved.</span>}
      </div>
    </form>
  );
}

type InputProps = { id: string; "aria-describedby": string; "aria-invalid"?: true };

function Field({
  name,
  label,
  help,
  error,
  check = false,
  children,
}: {
  name: string;
  label: string;
  help: string;
  error?: string;
  check?: boolean;
  children: (props: InputProps) => React.ReactNode;
}) {
  const base = useId();
  const id = `${base}-${name}`;
  const describedBy = `${id}-help${error ? ` ${id}-error` : ""}`;
  return (
    <div className="options-field" data-check={check}>
      <label htmlFor={id}>{label}</label>
      {children({
        id,
        "aria-describedby": describedBy,
        ...(error ? { "aria-invalid": true } : {}),
      })}
      <p id={`${id}-help`} className="options-help">
        {help}
      </p>
      {error && (
        <p id={`${id}-error`} role="alert" className="dialog-error">
          {error}
        </p>
      )}
    </div>
  );
}
