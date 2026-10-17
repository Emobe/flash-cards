import type { SearchMode, Target, TargetSummary } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "../core";

/** How long the selection rests before its counts are asked for, in ms. */
export const SUMMARY_DELAY_MS = 150;

const none: ReadonlySet<string> = new Set();

/**
 * The rows chosen in Browse (ADR 0014 decision 2): note IDs in Notes view, card IDs in Cards view.
 *
 * - `scope` is the query and the view. When it changes the selection is cleared: the rows it named
 *   are no longer the ones shown. A sort change keeps it.
 * - `selecting` is the phone's selection mode, entered with Select or a long press and left with ✕
 *   or Back. It can be on with nothing chosen. From 600 px there is no mode: `active` is true while
 *   something is chosen.
 * - Kept after an action, so several can be made on the same rows; the screen clears it after a
 *   delete.
 */
export function useSelection(scope: string) {
  const [ids, setIds] = useState<ReadonlySet<string>>(none);
  const [selecting, setSelecting] = useState(false);
  /** The last row toggled, where a Shift range starts. */
  const anchor = useRef<string | null>(null);

  const leave = useCallback(() => {
    setIds(none);
    setSelecting(false);
    anchor.current = null;
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: clears when the scope changes.
  useEffect(() => {
    setIds(none);
    anchor.current = null;
  }, [scope]);

  const toggle = useCallback((id: string) => {
    anchor.current = id;
    setIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  /** Chooses the loaded rows from the last one toggled to `id`, both included. */
  const range = useCallback(
    (id: string, loaded: string[]) => {
      const from = anchor.current === null ? -1 : loaded.indexOf(anchor.current);
      const to = loaded.indexOf(id);
      if (from === -1 || to === -1) {
        toggle(id);
        return;
      }
      const [start, end] = from < to ? [from, to] : [to, from];
      anchor.current = id;
      setIds((current) => new Set([...current, ...loaded.slice(start, end + 1)]));
    },
    [toggle],
  );

  /** The phone's long press: selection mode with that row chosen. */
  const enter = useCallback((id?: string) => {
    setSelecting(true);
    if (id !== undefined) {
      anchor.current = id;
      setIds((current) => new Set([...current, id]));
    }
  }, []);

  const replace = useCallback((all: string[]) => {
    anchor.current = null;
    setIds(new Set(all));
  }, []);

  return {
    ids,
    size: ids.size,
    selecting,
    active: selecting || ids.size > 0,
    toggle,
    range,
    enter,
    replace,
    leave,
  };
}

export function targetOf(ids: ReadonlySet<string>, mode: SearchMode): Target {
  return mode === "notes" ? { notes: [...ids] } : { cards: [...ids] };
}

/**
 * What the selection comes to (`describeTarget`): its notes and cards, how many of the cards are
 * suspended, and what a delete takes. Asked for once the selection rests, the latest answer wins,
 * and asked again when `version` changes (after an action). Null while nothing is chosen, while
 * asking, or when it could not be read (a row deleted elsewhere: the action then says so).
 */
export function useTargetSummary(
  ids: ReadonlySet<string>,
  mode: SearchMode,
  version: number,
): TargetSummary | null {
  const core = useCore();
  const [summary, setSummary] = useState<TargetSummary | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` asks again after a change.
  useEffect(() => {
    setSummary(null);
    if (ids.size === 0) return;
    let current = true;
    const timer = setTimeout(() => {
      core
        .call("describeTarget", { target: targetOf(ids, mode) })
        .then((answer) => {
          if (current) setSummary(answer);
        })
        .catch(() => {
          // Left null: the counts are not shown and the action reports what went wrong.
        });
    }, SUMMARY_DELAY_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [core, ids, mode, version]);
  return summary;
}
