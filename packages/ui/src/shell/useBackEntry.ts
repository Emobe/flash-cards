import { useEffect, useRef } from "react";
import { flushSync } from "react-dom";

/**
 * Back closes the top thing first. Each open window, layer or mode that Back should close holds a
 * history entry of its own, one above the other, so Back closes only the top one (a Move sheet
 * opened from the selection bar closes and the selection stays). An entry's state is
 * `{ fcDialog: n }`, where `n` is how many holders it covers.
 *
 * - Opening pushes an entry for the new holder.
 * - Back takes the top entry away: the holders above the entry now shown get `onBack`. A holder
 *   that stays (a window asking about unsaved changes) gets its entry pushed again.
 * - Closing by a button takes the entry back, once no holder needs it. Effects run twice in
 *   development, so that waits a tick, and a holder that comes straight back keeps the entry.
 * - An entry with no holder (left behind when a route changed under a window) is skipped when Back
 *   reaches it.
 */
type Holder = { onBack: () => void };

const stack: Holder[] = [];
let listening = false;
/** True from our own `history.go` until its `popstate`: the depth read meanwhile is stale. */
let going = false;

function depth(): number {
  const held = window.history.state?.fcDialog;
  return typeof held === "number" ? held : 0;
}

/** Pushes entries for holders that have none, and with `trim`, takes back entries no holder has. */
function sync(trim: boolean) {
  if (going) return;
  let at = depth();
  while (at < stack.length) {
    at += 1;
    window.history.pushState({ fcDialog: at }, "");
  }
  if (trim && at > stack.length) {
    going = true;
    window.history.go(stack.length - at);
  }
}

function onPop() {
  going = false;
  const at = depth();
  // Rendered at once, so a holder that closes is gone before the entries are counted again.
  flushSync(() => {
    for (const holder of stack.slice(at).reverse()) holder.onBack();
  });
  setTimeout(() => sync(true), 0);
}

function acquire(holder: Holder) {
  if (!listening) {
    // Stays for the life of the page, so an entry left behind is skipped even with nothing open.
    window.addEventListener("popstate", onPop);
    listening = true;
  }
  stack.push(holder);
  sync(false);
}

function release(holder: Holder) {
  const index = stack.indexOf(holder);
  if (index !== -1) stack.splice(index, 1);
  setTimeout(() => sync(true), 0);
}

/**
 * Holds a history entry while `active`, so Back (the Android button, the browser's) calls `onBack`
 * instead of leaving the screen. `onBack` may leave things as they are; Back then works again.
 */
export function useBackEntry(active: boolean, onBack: () => void) {
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  useEffect(() => {
    if (!active) return;
    const holder = { onBack: () => onBackRef.current() };
    acquire(holder);
    return () => release(holder);
  }, [active]);
}
