import type { EditorView } from "prosemirror-view";
import { type ReactNode, useRef } from "react";
import {
  activeList,
  type ClozeKind,
  insertCloze,
  isMarkActive,
  toggleBold,
  toggleItalic,
  toggleList,
} from "./commands";
import { types } from "./schema";

const LONG_PRESS_MS = 500;

function ToolButton({
  label,
  title,
  pressed,
  onRun,
  onLongPress,
  children,
}: {
  label: string;
  title: string;
  pressed?: boolean;
  onRun: () => void;
  onLongPress?: () => void;
  children: ReactNode;
}) {
  const timer = useRef<number | undefined>(undefined);
  const longPressed = useRef(false);

  return (
    <button
      type="button"
      className="tool-button"
      aria-label={label}
      title={title}
      aria-pressed={pressed}
      // The editor keeps focus (and the keyboard stays open) while a button is pressed.
      onMouseDown={(event) => event.preventDefault()}
      onPointerDown={(event) => {
        event.preventDefault();
        longPressed.current = false;
        if (onLongPress) {
          timer.current = window.setTimeout(() => {
            longPressed.current = true;
            onLongPress();
          }, LONG_PRESS_MS);
        }
      }}
      onPointerUp={() => window.clearTimeout(timer.current)}
      onPointerLeave={() => window.clearTimeout(timer.current)}
      onPointerCancel={() => window.clearTimeout(timer.current)}
      onContextMenu={(event) => event.preventDefault()}
      onClick={() => {
        if (longPressed.current) return;
        onRun();
      }}
    >
      {children}
    </button>
  );
}

/**
 * Formatting for the field being edited, in the screen's bottom action (ADR 0011 decision 1). It
 * reads the view's state when it renders, so the screen re-renders it after each transaction.
 */
export function Toolbar({
  view,
  cloze,
  highestCloze,
}: {
  view: EditorView | null;
  cloze: boolean;
  highestCloze: () => number;
}) {
  const state = view?.state;
  const list = state ? activeList(state) : null;

  function run(command: typeof toggleBold) {
    if (!view) return;
    view.focus();
    command(view.state, view.dispatch, view);
  }
  const clozeWith = (kind: ClozeKind) => run(insertCloze(kind, highestCloze));

  return (
    <div className="add-tools" role="toolbar" aria-label="Formatting">
      <ToolButton
        label="Bold"
        title="Bold (Ctrl+B)"
        pressed={state ? isMarkActive(state, types.bold) : false}
        onRun={() => run(toggleBold)}
      >
        <b>B</b>
      </ToolButton>
      <ToolButton
        label="Italic"
        title="Italic (Ctrl+I)"
        pressed={state ? isMarkActive(state, types.italic) : false}
        onRun={() => run(toggleItalic)}
      >
        <i>I</i>
      </ToolButton>
      <ToolButton
        label="Bullet list"
        title="Bullet list"
        pressed={list === types.bulletList}
        onRun={() => run(toggleList(types.bulletList))}
      >
        •
      </ToolButton>
      <ToolButton
        label="Numbered list"
        title="Numbered list"
        pressed={list === types.orderedList}
        onRun={() => run(toggleList(types.orderedList))}
      >
        1.
      </ToolButton>
      {cloze && (
        <ToolButton
          label="Cloze"
          title="Hide this text on a card (Ctrl+Shift+C). Hold for the same number again."
          onRun={() => clozeWith("new")}
          onLongPress={() => clozeWith("same")}
        >
          <span className="tool-word">Cloze</span>
        </ToolButton>
      )}
    </div>
  );
}
