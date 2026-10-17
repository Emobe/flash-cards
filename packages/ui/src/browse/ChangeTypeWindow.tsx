import {
  type ChangeNoteTypeOutput,
  CoreError,
  type NoteTypeSummary,
  type Target,
} from "core-client";
import { useCallback, useEffect, useId, useState } from "react";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { WarningIcon } from "../shell/icons";
import { BottomAction } from "../shell/slot";
import { carries, defaultMaps, describeChange, type Maps } from "./changeType";
import "../screens/add/add.css";
import "./change-type.css";

const TITLE = "Change note type";

/**
 * Change the note type of notes (ADR 0014 decision 6): the new type, which old field fills each new
 * field, and which old card gives each new card its schedule, with a sentence saying what it will
 * do before Change. `noteTypeId` is the type the notes share, `null` for a mixed selection.
 */
export function ChangeTypeWindow({
  target,
  notes,
  noteTypeId,
  onDone,
  onClose,
}: {
  target: Target;
  /** How many notes the target reaches. */
  notes: number;
  noteTypeId: string | null;
  onDone: (result: ChangeNoteTypeOutput) => void;
  onClose: () => void;
}) {
  const core = useCore();
  const [types, setTypes] = useState<NoteTypeSummary[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [toId, setToId] = useState<string | null>(null);
  const [maps, setMaps] = useState<Maps>({ fields: {}, templates: {} });
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const typeId = useId();

  const load = useCallback(() => {
    if (noteTypeId === null) return;
    setFailed(null);
    core
      .call("getNoteTypes", null)
      .then((out) => setTypes(out.noteTypes))
      .catch((failure: unknown) =>
        setFailed(
          failure instanceof CoreError ? failure.message : "The note types could not be loaded.",
        ),
      );
  }, [core, noteTypeId]);
  useEffect(load, [load]);

  const from = types?.find((t) => t.id === noteTypeId) ?? null;
  const choices = types?.filter((t) => t.id !== noteTypeId) ?? [];
  const to = choices.find((t) => t.id === toId) ?? null;

  function choose(id: string) {
    setToId(id);
    setProblem(null);
    const next = choices.find((t) => t.id === id);
    if (from && next) setMaps(defaultMaps(from, next));
  }

  // The first other type, once they are loaded.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only the first load picks one.
  useEffect(() => {
    if (types && from && toId === null && choices[0]) choose(choices[0].id);
  }, [types]);

  async function change() {
    if (!from || !to || busy) return;
    setBusy(true);
    setProblem(null);
    try {
      const out = await core.call("changeNoteType", {
        target,
        noteTypeId: to.id,
        fields: to.fields.map((f) => ({ fieldId: f.id, from: maps.fields[f.id] || null })),
        templates: carries(from, to)
          ? to.templates.map((t) => ({ templateId: t.id, from: maps.templates[t.id] || null }))
          : [],
      });
      onDone(out);
    } catch (failure) {
      setProblem(failure instanceof CoreError ? failure.message : "That did not work. Try again.");
    } finally {
      setBusy(false);
    }
  }

  if (noteTypeId === null) {
    return (
      <Dialog title={TITLE} variant="sheet" onClose={onClose}>
        <p>Select notes of one note type to change it.</p>
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog title={TITLE} variant="layer" onClose={onClose}>
      {failed && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{failed}</p>
          <button type="button" className="button" onClick={load}>
            Try again
          </button>
        </div>
      )}
      {!failed && !types && <p role="status">Opening...</p>}
      {types && !from && <p role="alert">That note type no longer exists.</p>}
      {types && from && choices.length === 0 && <p>There is no other note type to change to.</p>}
      {types && from && to && (
        <form
          className="add change-type"
          aria-label="Change note type"
          onSubmit={(event) => {
            event.preventDefault();
            void change();
          }}
        >
          <div className="add-choice">
            <label htmlFor={typeId} className="add-label">
              From {from.name} to
            </label>
            <select id={typeId} value={to.id} onChange={(e) => choose(e.target.value)}>
              {choices.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
          </div>

          <fieldset className="change-type-group">
            <legend className="add-label">Fields</legend>
            {to.fields.map((field) => (
              <Pick
                key={field.id}
                label={field.name}
                nothing="Nothing"
                value={maps.fields[field.id] ?? ""}
                options={from.fields}
                onChange={(value) =>
                  setMaps((m) => ({ ...m, fields: { ...m.fields, [field.id]: value } }))
                }
              />
            ))}
          </fieldset>

          {carries(from, to) && to.kind === "standard" && (
            <fieldset className="change-type-group">
              <legend className="add-label">Cards keep the schedule of</legend>
              {to.templates.map((template) => (
                <Pick
                  key={template.id}
                  label={template.name}
                  nothing="Start as new"
                  value={maps.templates[template.id] ?? ""}
                  options={from.templates}
                  taken={Object.entries(maps.templates)
                    .filter(([id, old]) => id !== template.id && old)
                    .map(([, old]) => old)}
                  onChange={(value) =>
                    setMaps((m) => ({
                      ...m,
                      templates: { ...m.templates, [template.id]: value },
                    }))
                  }
                />
              ))}
            </fieldset>
          )}

          <p className="picker-note change-type-summary" role="status">
            {describeChange(notes, from, to, maps)}
          </p>
          <p className="picker-note">
            The old fields and cards are kept. You can undo this right after.
          </p>
          {problem && (
            <p role="alert" className="add-error">
              <WarningIcon />
              <span>{problem}</span>
            </p>
          )}
          <BottomAction>
            <div className="add-bar">
              <button
                type="button"
                className="button button-primary add-submit"
                disabled={busy}
                onClick={() => void change()}
              >
                Change
              </button>
            </div>
          </BottomAction>
        </form>
      )}
    </Dialog>
  );
}

/** One new part and the old part that fills it. `taken` old parts are offered by another row. */
function Pick({
  label,
  nothing,
  value,
  options,
  taken = [],
  onChange,
}: {
  label: string;
  nothing: string;
  value: string;
  options: { id: string; name: string }[];
  taken?: string[];
  onChange: (value: string) => void;
}) {
  const id = useId();
  return (
    <div className="change-type-row">
      <label htmlFor={id}>{label}</label>
      <select id={id} value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{nothing}</option>
        {options.map((o) => (
          <option key={o.id} value={o.id} disabled={taken.includes(o.id)}>
            {o.name}
          </option>
        ))}
      </select>
    </div>
  );
}
