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
