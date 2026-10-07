import { createContext, type ReactNode, useContext } from "react";
import { createPortal } from "react-dom";

/** The element the shell keeps above the keyboard (and above the bottom bar). */
export const BottomActionSlot = createContext<HTMLElement | null>(null);

/**
 * Pins its children to the bottom of the screen, above the on-screen keyboard (ADR 0010 decision
 * 4). A screen puts its primary action here and never does the keyboard arithmetic itself.
 */
export function BottomAction({ children }: { children: ReactNode }) {
  const slot = useContext(BottomActionSlot);
  return slot ? createPortal(children, slot) : null;
}

/** How far the on-screen keyboard covers the page, in px. Provided by the shell. */
export const KeyboardInsetContext = createContext(0);

/**
 * The shell's keyboard inset, for a screen that has to keep something in view when the keyboard
 * opens or closes (the Add screen scrolls the caret). The screen still never does the arithmetic:
 * the shell already shrinks the page to what is visible.
 */
export function useShellKeyboardInset(): number {
  return useContext(KeyboardInsetContext);
}
