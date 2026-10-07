import type { EditorView } from "prosemirror-view";
import { type ReactNode, useEffect, useRef, useState } from "react";
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
  disabled,
  onRun,
  onLongPress,
  children,
}: {
  label: string;
  title: string;
  pressed?: boolean;
  disabled?: boolean;
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
      disabled={disabled}
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
/** True on a touch screen, where "Take photo" makes sense (not a platform check, ADR 0010). */
function useCoarsePointer(): boolean {
  const query = "(pointer: coarse)";
  const [coarse, setCoarse] = useState(() => window.matchMedia?.(query).matches ?? false);
  useEffect(() => {
    const list = window.matchMedia?.(query);
    if (!list) return;
    const update = () => setCoarse(list.matches);
    list.addEventListener("change", update);
    return () => list.removeEventListener("change", update);
  }, []);
  return coarse;
}

export function Toolbar({
  view,
  cloze,
  highestCloze,
  onFiles,
  busy = false,
}: {
  view: EditorView | null;
  cloze: boolean;
  highestCloze: () => number;
  /** Files picked with the Image, Take photo or Sound button. */
  onFiles: (kind: "image" | "sound", files: File[]) => void;
  /** Files are being prepared and stored: the buttons wait. */
  busy?: boolean;
}) {
  const coarse = useCoarsePointer();
  const imageInput = useRef<HTMLInputElement>(null);
  const cameraInput = useRef<HTMLInputElement>(null);
  const soundInput = useRef<HTMLInputElement>(null);

  function picked(kind: "image" | "sound") {
    return (event: React.ChangeEvent<HTMLInputElement>) => {
      const files = [...(event.target.files ?? [])];
      // So the same file can be picked again.
      event.target.value = "";
      if (files.length > 0) onFiles(kind, files);
    };
  }
  const state = view?.state;
  const list = state ? activeList(state) : null;

  function run(command: typeof toggleBold) {
    if (!view) return;
    view.focus();
    command(view.state, view.dispatch, view);
  }
  const clozeWith = (kind: ClozeKind) => run(insertCloze(kind, highestCloze));
  const choose = (input: React.RefObject<HTMLInputElement | null>) => () => input.current?.click();

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
      <ToolButton label="Image" title="Add a picture" disabled={busy} onRun={choose(imageInput)}>
        <span className="tool-word">Image</span>
      </ToolButton>
      {coarse && (
        <ToolButton
          label="Take photo"
          title="Take a photo"
          disabled={busy}
          onRun={choose(cameraInput)}
        >
          <span className="tool-word">Photo</span>
        </ToolButton>
      )}
      <ToolButton label="Sound" title="Add a sound" disabled={busy} onRun={choose(soundInput)}>
        <span className="tool-word">Sound</span>
      </ToolButton>
      <input
        ref={imageInput}
        type="file"
        accept="image/*"
        multiple
        hidden
        aria-hidden="true"
        tabIndex={-1}
        onChange={picked("image")}
      />
      <input
        ref={cameraInput}
        type="file"
        accept="image/*"
        capture="environment"
        hidden
        aria-hidden="true"
        tabIndex={-1}
        onChange={picked("image")}
      />
      <input
        ref={soundInput}
        type="file"
        accept="audio/*"
        hidden
        aria-hidden="true"
        tabIndex={-1}
        onChange={picked("sound")}
      />
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
