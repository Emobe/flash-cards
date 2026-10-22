import { type Dispatch, useLayoutEffect, useRef, useState } from "react";
import { Dialog } from "../../components/Dialog";
import { WarningIcon } from "../../shell/icons";
import { BottomAction } from "../../shell/slot";
import { useMediaQuery, useWideLayout } from "../../shell/useMediaQuery";
import { useTheme } from "../../theme";
import { type Action, type DraftState, toDraft } from "./draft";
import { type Side, TemplatePreview, usePreview } from "./TemplatePreview";
import { insertAt, offsetOf, wrapIf } from "./templateText";

/** From here the text box and the preview sit side by side (ADR 0015 decision 4). */
export const TWO_COLUMNS_QUERY = "(min-width: 840px)";

export type Tab = "front" | "back" | "styling";

const TAB_LABEL: Record<Tab, string> = { front: "Front", back: "Back", styling: "Styling" };

type Selection = { start: number; end: number };

/**
 * The template editor (step 3.4c, ADR 0015 decisions 3 and 4): a full-screen layer over the note
 * type screen with tabs Front, Back and Styling, an Insert row and a live preview. It changes the
 * screen's draft as you type, so Done only closes it, and nothing is written until Save.
 */
export function TemplateEditor(props: {
  noteTypeId: string;
  cloze: boolean;
  draft: DraftState;
  dispatch: Dispatch<Action>;
  /** The key of the card being edited. */
  templateKey: string;
  initialTab: Tab;
  onClose: () => void;
}) {
  const { noteTypeId, cloze, draft, dispatch, templateKey, onClose } = props;
  const { effective } = useTheme();
  const wide = useWideLayout();
  const twoColumns = useMediaQuery(TWO_COLUMNS_QUERY);
  const index = Math.max(
    0,
    draft.templates.findIndex((t) => t.key === templateKey),
  );
  const template = draft.templates[index];

  const [tab, setTab] = useState<Tab>(props.initialTab);
  const [side, setSide] = useState<Side>(props.initialTab === "front" ? "front" : "back");
  /** Under 840 px only one of the two is shown. */
  const [view, setView] = useState<"edit" | "preview">("edit");
  const [picking, setPicking] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const pending = useRef<Selection | null>(null);
  const saved = useRef<Selection>({ start: 0, end: 0 });
  /** Changes to make the box apply a cursor that was set without any other change to show. */
  const [, setTick] = useState(0);

  const preview = usePreview({
    noteTypeId,
    draft: toDraft(draft),
    templateIndex: index,
    cloze,
  });

  const text = tab === "styling" ? draft.css : (template?.[tab] ?? "");

  // A tab shows the side it edits; Styling shows the back, where most styling is seen.
  function choose(next: Tab) {
    setTab(next);
    setSide(next === "front" ? "front" : "back");
  }

  // The cursor is put back after the text changed or the box came back.
  useLayoutEffect(() => {
    const want = pending.current;
    const element = box.current;
    if (!want || !element || element.hidden) return;
    pending.current = null;
    element.focus();
    element.setSelectionRange(want.start, want.end);
  });

  function write(value: string) {
    if (tab === "styling") dispatch({ type: "css", css: value });
    else dispatch({ type: "templateText", key: templateKey, side: tab, text: value });
  }

  function remember() {
    const element = box.current;
    if (element) saved.current = { start: element.selectionStart, end: element.selectionEnd };
  }

  function apply(edit: (text: string, start: number, end: number) => ReturnType<typeof insertAt>) {
    const element = box.current;
    if (!element) return;
    const start = element.selectionStart;
    const end = element.selectionEnd;
    const done = edit(element.value, start, end);
    pending.current = { start: done.start, end: done.end };
    write(done.text);
    // Setting the same text again (a no-op edit) would not render, so put the cursor back now.
    if (done.text === element.value) {
      pending.current = null;
      element.setSelectionRange(done.start, done.end);
    }
  }

  function showPreview() {
    remember();
    box.current?.blur();
    setView("preview");
  }

  function showEdit() {
    pending.current = { ...saved.current };
    setView("edit");
  }

  function goTo(mistakeSide: Side, line: number, column: number) {
    const source = template?.[mistakeSide] ?? "";
    const at = offsetOf(source, line, column);
    choose(mistakeSide);
    pending.current = { start: at, end: at };
    setView("edit");
    setTick((n) => n + 1);
  }

  const fieldNames = draft.fields.map((f) => f.name);
  // Typing in the box must not lose its selection when a button is pressed.
  const keepFocus = (e: { preventDefault: () => void }) => e.preventDefault();

  const insertRow =
    tab === "styling" ? null : (
      <fieldset className="insert-row" aria-label="Insert">
        {tab === "back" && (
          <button
            type="button"
            className="button insert-chip"
            onMouseDown={keepFocus}
            onClick={() => apply((t, s, e) => insertAt(t, s, e, "{{FrontSide}}"))}
          >
            FrontSide
          </button>
        )}
        {fieldNames.map((name) => (
          <button
            key={`f${name}`}
            type="button"
            className="button insert-chip"
            aria-label={`Insert ${name}`}
            onMouseDown={keepFocus}
            onClick={() => apply((t, s, e) => insertAt(t, s, e, `{{${name}}}`))}
          >
            {name}
          </button>
        ))}
        {cloze &&
          fieldNames.map((name) => (
            <button
              key={`c${name}`}
              type="button"
              className="button insert-chip"
              aria-label={`Insert cloze ${name}`}
              onMouseDown={keepFocus}
              onClick={() => apply((t, s, e) => insertAt(t, s, e, `{{cloze:${name}}}`))}
            >
              cloze:{name}
            </button>
          ))}
        <button
          type="button"
          className="button insert-chip"
          onMouseDown={keepFocus}
          onClick={() => {
            remember();
            setPicking(true);
          }}
        >
          Show if…
        </button>
      </fieldset>
    );

  const switcher = !twoColumns && (
    <fieldset className="edit-preview" aria-label="View">
      <button
        type="button"
        className="button"
        aria-pressed={view === "edit"}
        onMouseDown={keepFocus}
        onClick={showEdit}
      >
        Edit
      </button>
      <button
        type="button"
        className="button"
        aria-pressed={view === "preview"}
        onMouseDown={keepFocus}
        onClick={showPreview}
      >
        Preview
      </button>
    </fieldset>
  );

  const mistakes = preview.problems.flatMap((group) =>
    group.mistakes.map((m) => ({ side: group.side, ...m })),
  );

  // Under 600 px the Insert row and the switch sit above the keyboard.
  const compact = !wide;
  const editing = twoColumns || view === "edit";
  const bar = compact ? (
    <BottomAction>
      <div className="template-bar">
        {editing && insertRow}
        {switcher}
      </div>
    </BottomAction>
  ) : null;

  return (
    <Dialog
      title={tab === "styling" ? "Styling" : `Edit ${template?.name ?? "card"}`}
      variant="layer"
      onClose={onClose}
    >
      <div className="template-editor">
        <div className="template-head">
          <div className="template-tabs" role="tablist" aria-label="Part of the card">
            {(["front", "back", "styling"] as const).map((t) => (
              <button
                key={t}
                type="button"
                role="tab"
                className="button"
                aria-selected={tab === t}
                onClick={() => choose(t)}
              >
                {TAB_LABEL[t]}
              </button>
            ))}
          </div>
          {!compact && !twoColumns && switcher}
          <button type="button" className="button button-primary" onClick={onClose}>
            Done
          </button>
        </div>

        <div className="template-columns" data-two={twoColumns}>
          <div className="template-edit" hidden={!editing}>
            {!compact && insertRow}
            <textarea
              ref={box}
              className="template-box"
              aria-label={`${TAB_LABEL[tab]} template`}
              value={text}
              wrap={wide ? "off" : "soft"}
              autoCapitalize="off"
              autoCorrect="off"
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => write(e.target.value)}
              onSelect={remember}
              onBlur={remember}
            />
            {tab !== "styling" && (
              <Problems
                mistakes={mistakes}
                unknown={preview.unknownFields}
                onGo={(m) => goTo(m.side, m.line, m.column)}
              />
            )}
          </div>
          <div className="template-side" hidden={!twoColumns && view !== "preview"}>
            <TemplatePreview
              preview={preview}
              side={side}
              onSide={setSide}
              theme={effective}
              cloze={cloze}
            />
          </div>
        </div>
      </div>
      {bar}
      {picking && (
        <ShowIf
          names={fieldNames}
          onClose={() => setPicking(false)}
          onPick={(name) => {
            setPicking(false);
            // The window closing gives focus back to the box; the cursor is where it was.
            pending.current = { ...saved.current };
            const element = box.current;
            if (element) element.setSelectionRange(saved.current.start, saved.current.end);
            apply((t, s, e) => wrapIf(t, s, e, name));
          }}
        />
      )}
    </Dialog>
  );
}

function Problems(props: {
  mistakes: { side: Side; line: number; column: number; message: string }[];
  unknown: string[];
  onGo: (mistake: { side: Side; line: number; column: number }) => void;
}) {
  if (props.mistakes.length === 0 && props.unknown.length === 0) return null;
  return (
    <ul className="template-problems">
      {props.mistakes.map((m) => (
        <li key={`${m.side}:${m.line}:${m.column}:${m.message}`}>
          <button type="button" className="problem" onClick={() => props.onGo(m)}>
            <WarningIcon />
            <span>
              {m.side === "front" ? "Front" : "Back"}, line {m.line}, column {m.column}: {m.message}
            </span>
          </button>
        </li>
      ))}
      {props.unknown.map((name) => (
        <li key={`u:${name}`} className="problem-warning">
          This template uses <code>{name}</code>, which is not a field. It will show nothing.
        </li>
      ))}
    </ul>
  );
}

function ShowIf(props: { names: string[]; onClose: () => void; onPick: (name: string) => void }) {
  return (
    <Dialog title="Show only if a field has text" variant="sheet" onClose={props.onClose}>
      <div className="part-sheet">
        {props.names.map((name) => (
          <button key={name} type="button" className="button" onClick={() => props.onPick(name)}>
            {name}
          </button>
        ))}
        <button type="button" className="button" onClick={props.onClose}>
          Cancel
        </button>
      </div>
    </Dialog>
  );
}
