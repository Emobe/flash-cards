import { useEffect, useRef } from "react";

/** How long a press must last to count as long, in ms (ADR 0014 decision 2). */
export const LONG_PRESS_MS = 500;
/** How far a finger may move before the press becomes a scroll, in px. */
const SLOP_PX = 10;

/**
 * A long press on a row, for the phone's selection mode. Moving, lifting or the browser taking the
 * gesture (a scroll) cancels it. The click that follows a long press is swallowed, so the row does
 * not also open. Off when `enabled` is false (from 600 px, where a mouse has checkboxes, and in
 * selection mode, where a tap already toggles).
 */
export function useLongPress(enabled: boolean, onLongPress: () => void) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const start = useRef<{ x: number; y: number } | null>(null);
  const fired = useRef(false);
  const onLongPressRef = useRef(onLongPress);
  onLongPressRef.current = onLongPress;

  const cancel = () => {
    clearTimeout(timer.current);
    timer.current = undefined;
    start.current = null;
  };
  useEffect(() => () => clearTimeout(timer.current), []);

  // Swallows the click that ends a long press even once the press has turned selection mode on
  // (which turns the press itself off), or the row would be toggled straight back.
  const onClickCapture = (event: React.MouseEvent) => {
    if (!fired.current) return;
    fired.current = false;
    event.preventDefault();
    event.stopPropagation();
  };
  if (!enabled) return { onClickCapture };
  return {
    onPointerDown: (event: React.PointerEvent) => {
      if (event.button !== 0) return;
      fired.current = false;
      start.current = { x: event.clientX, y: event.clientY };
      clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = undefined;
        start.current = null;
        fired.current = true;
        onLongPressRef.current();
      }, LONG_PRESS_MS);
    },
    onPointerMove: (event: React.PointerEvent) => {
      const at = start.current;
      if (at && Math.hypot(event.clientX - at.x, event.clientY - at.y) > SLOP_PX) cancel();
    },
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerLeave: cancel,
    onClickCapture,
  };
}
