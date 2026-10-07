import { useEffect, useRef } from "react";

/** True when typing in `target` must not trigger a shortcut. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return true;
  return target.closest('[contenteditable=""], [contenteditable="true"]') !== null;
}

const CHORD_MS = 1500;

/**
 * `g` then a letter goes to a destination, `?` opens the list (ADR 0010 decision 2). Keys typed
 * into a text field, or with Ctrl, Alt or Meta held, are ignored. Keys pressed inside a card frame
 * never get here (ADR 0005).
 */
export function useShortcuts({
  enabled,
  goTo,
  showHelp,
}: {
  enabled: boolean;
  /** Called with the letter after `g`. Returns whether the letter was a destination. */
  goTo: (letter: string) => boolean;
  showHelp: () => void;
}) {
  const pending = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!enabled) return;
    const clear = () => {
      window.clearTimeout(pending.current);
      pending.current = undefined;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.altKey || event.metaKey || event.isComposing) return;
      if (isTypingTarget(event.target)) return;
      if (pending.current !== undefined) {
        clear();
        if (goTo(event.key.toLowerCase())) event.preventDefault();
        return;
      }
      if (event.key === "g") {
        pending.current = window.setTimeout(clear, CHORD_MS);
      } else if (event.key === "?") {
        event.preventDefault();
        showHelp();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      clear();
    };
  }, [enabled, goTo, showHelp]);
}
