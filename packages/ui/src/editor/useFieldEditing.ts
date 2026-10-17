import { CoreError } from "core-client";
import type { EditorView } from "prosemirror-view";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "../core";
import { useShellKeyboardInset } from "../shell/slot";
import { insertMedia } from "./commands";
import { MediaRefused, prepareImage, prepareSound } from "./media";

/**
 * What a screen with field editors and the toolbar shares (the Add screen, the note editor): the
 * editor the toolbar acts on, the caret kept above the keyboard and the toolbar, and pictures and
 * sounds stored as soon as they are picked (ADR 0011 decision 3). `report` shows progress and
 * problems where the screen shows them.
 */
export function useFieldEditing(report: {
  status: (text: string) => void;
  error: (text: string | null) => void;
}) {
  const core = useCore();
  // The editor the toolbar acts on, and a counter that re-renders the toolbar after each change.
  const [active, setActive] = useState<EditorView | null>(null);
  const [, setTick] = useState(0);
  const [mediaBusy, setMediaBusy] = useState(false);
  const views = useRef(new Map<string, EditorView>());
  // Files picked or fetched in this visit, so a picture just added is not read back from the core.
  const mediaCache = useRef(new Map<string, Blob>());
  const keyboardInset = useShellKeyboardInset();
  const reportRef = useRef(report);
  reportRef.current = report;

  // ---- Keeping the caret in view above the keyboard and the toolbar ----

  // biome-ignore lint/correctness/useExhaustiveDependencies: only a change of the keyboard inset should scroll.
  useEffect(() => {
    if (active?.hasFocus()) active.dispatch(active.state.tr.scrollIntoView());
  }, [keyboardInset]);

  const onFocus = useCallback((view: EditorView) => {
    setActive(view);
    // After the keyboard has had a moment to come up; the inset effect covers the rest.
    window.requestAnimationFrame(() => {
      if (view.hasFocus()) view.dispatch(view.state.tr.scrollIntoView());
    });
  }, []);

  const onTransaction = useCallback(
    (view: EditorView) => {
      if (view === active) setTick((n) => n + 1);
    },
    [active],
  );

  /** For `FieldEditor`'s `onReady`: the views by the screen's key, for focus and media. */
  const ready = useCallback((key: string, view: EditorView | null) => {
    if (view) views.current.set(key, view);
    else views.current.delete(key);
  }, []);

  // ---- Pictures and sounds ----

  const loadMedia = useCallback(
    async (name: string) => {
      const cached = mediaCache.current.get(name);
      if (cached) return cached;
      const { output, bytes } = await core.call("getMedia", { name });
      // A copy, because the reply may be a view on a larger buffer.
      const blob = new Blob([new Uint8Array(bytes)], { type: output.contentType });
      mediaCache.current.set(name, blob);
      return blob;
    },
    [core],
  );

  async function attach(kind: "image" | "sound", files: File[]) {
    // The editor last used, unless a change of note type has removed it.
    const view = active?.dom.isConnected ? active : [...views.current.values()][0];
    if (!view || mediaBusy) return;
    const { status, error } = reportRef.current;
    setMediaBusy(true);
    error(null);
    status(files.length === 1 ? "Adding the file..." : `Adding ${files.length} files...`);
    const problems: string[] = [];
    for (const file of files) {
      try {
        const prepared = kind === "image" ? await prepareImage(file) : await prepareSound(file);
        // Before the call: the web client hands the bytes to its worker, which detaches them here.
        const shown = new Blob([new Uint8Array(prepared.bytes)], { type: prepared.type });
        const added = await core.call(
          "addMedia",
          { name: prepared.name },
          { bytes: prepared.bytes },
        );
        mediaCache.current.set(added.name, shown);
        if (view.dom.isConnected) insertMedia(kind, added.name)(view.state, view.dispatch);
        if (prepared.warning) problems.push(prepared.warning);
      } catch (failure) {
        if (failure instanceof MediaRefused || failure instanceof CoreError) {
          problems.push(failure.message);
        } else {
          // Details go to the log, never to the person.
          console.error("Could not add a file:", failure);
          problems.push("The file could not be added. Try again.");
        }
      }
    }
    status("");
    error(problems.length > 0 ? problems.join(" ") : null);
    setMediaBusy(false);
    if (view.dom.isConnected) view.focus();
  }

  return {
    active,
    views,
    mediaCache,
    mediaBusy,
    loadMedia,
    attach,
    /** Props for each `FieldEditor`. */
    editorProps: { onFocus, onTransaction, loadMedia },
    ready,
  };
}
