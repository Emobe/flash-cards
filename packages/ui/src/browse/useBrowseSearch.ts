import {
  CoreError,
  type SearchMode,
  type SearchResultRow,
  type SearchSort,
  type SearchSortKey,
} from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "../core";
import {
  type AddedChoice,
  type AnsweredChoice,
  addedTerms,
  answeredTerms,
  type DueChoice,
  dueTerms,
  type Filters,
  hasText,
  noFilters,
  type StateChoice,
  stateChoices,
  toQuery,
} from "./query";

export const PAGE = 100;
/** The core answers at most this many rows in one call. */
const MAX_LIMIT = 1000;
/** How long typing waits before searching, and how long a search runs before "Searching…" shows. */
export const DELAY_MS = 300;
const STORAGE_KEY = "fc.browse";

export const sortKeys: SearchSortKey[] = [
  "created",
  "due",
  "field",
  "deck",
  "note",
  "difficulty",
  "stability",
  "lapses",
  "reviews",
  "random",
];

/** What the screen shows, remembered per device (ADR 0013 decision 6). */
export type BrowseState = {
  filters: Filters;
  text: string;
  sort: SearchSort;
  mode: SearchMode;
  /** For the Shuffle sort: one order while paging, a new one each time Shuffle is chosen. */
  seed: number;
};

export const defaultState: BrowseState = {
  filters: noFilters,
  text: "",
  sort: { key: "created", descending: true },
  mode: "notes",
  seed: 0,
};

const isString = (v: unknown): v is string => typeof v === "string";
const strings = (v: unknown): string[] | null => (Array.isArray(v) && v.every(isString) ? v : null);
const oneOf = <K extends string>(v: unknown, keys: readonly K[]): K | null =>
  keys.includes(v as K) ? (v as K) : null;

/** The stored state, with anything missing or broken read as the default. */
export function readState(): BrowseState {
  let stored: Record<string, unknown> = {};
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (value && typeof value === "object") stored = value as Record<string, unknown>;
  } catch {
    // Unreadable or blocked: the default.
  }
  const f = (stored.filters && typeof stored.filters === "object" ? stored.filters : {}) as Record<
    string,
    unknown
  >;
  const states = strings(f.states)?.filter((s): s is StateChoice =>
    stateChoices.includes(s as StateChoice),
  );
  const sort = (stored.sort && typeof stored.sort === "object" ? stored.sort : {}) as Record<
    string,
    unknown
  >;
  const key = oneOf(sort.key, sortKeys);
  return {
    filters: {
      decks: strings(f.decks) ?? [],
      tags: strings(f.tags) ?? [],
      noteTypes: strings(f.noteTypes) ?? [],
      states: states ?? [],
      due: oneOf(f.due, Object.keys(dueTerms) as DueChoice[]),
      added: oneOf(f.added, Object.keys(addedTerms) as AddedChoice[]),
      answered: oneOf(f.answered, Object.keys(answeredTerms) as AnsweredChoice[]),
    },
    text: isString(stored.text) ? stored.text : "",
    sort:
      key && typeof sort.descending === "boolean"
        ? { key, descending: sort.descending }
        : defaultState.sort,
    mode: oneOf(stored.mode, ["notes", "cards"] as const) ?? defaultState.mode,
    seed:
      typeof stored.seed === "number" && Number.isInteger(stored.seed) && stored.seed >= 0
        ? stored.seed % 2 ** 32
        : 0,
  };
}

function writeState(state: BrowseState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Remembered only until the screen closes.
  }
}

export function newSeed(): number {
  return Math.floor(Math.random() * 2 ** 32);
}

/** The rows on screen, for one exact search. */
type Shown = { key: string; total: number; rows: SearchResultRow[] };

function searchKey(state: BrowseState): string {
  const { sort, mode, seed } = state;
  return JSON.stringify([toQuery(state.filters, state.text), sort, mode, seed]);
}

function message(error: unknown): string {
  return error instanceof CoreError ? error.message : "Something went wrong while searching.";
}

/**
 * The Browse screen's search (ADR 0013 decision 5). Typing searches 300 ms after the last change,
 * anything else at once. One search runs at a time and the newest state wins: a change while one
 * runs queues one more run, and a result for an older state is dropped. Nothing is cancelled.
 */
export function useBrowseSearch() {
  const core = useCore();
  const [state, setState] = useState<BrowseState>(readState);
  const [shown, setShown] = useState<Shown | null>(null);
  /** A mistake in the box, or a search the core refused. The old rows stay, dimmed. */
  const [problem, setProblem] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  const stateRef = useRef(state);
  stateRef.current = state;
  const shownRef = useRef(shown);
  shownRef.current = shown;
  /** Counts requests, so a run can tell whether a newer one came while it ran. */
  const wanted = useRef(0);
  const running = useRef(false);
  const debounce = useRef<number | undefined>(undefined);
  const live = useRef(true);

  const run = useCallback(async () => {
    if (running.current) return;
    running.current = true;
    const slow = window.setTimeout(() => live.current && setSearching(true), DELAY_MS);
    try {
      for (;;) {
        const request = wanted.current;
        const current = stateRef.current;
        const key = searchKey(current);
        let outcome: { shown: Shown } | { problem: string };
        try {
          const check = hasText(current.text)
            ? await core.call("checkQuery", { query: current.text })
            : { problem: null };
          if (check.problem) {
            outcome = { problem: check.problem.message };
          } else {
            // A refresh of the same search keeps as many rows as are on screen.
            const before = shownRef.current;
            const keep = before?.key === key ? before.rows.length : 0;
            const results = await core.call("search", {
              query: toQuery(current.filters, current.text),
              sort: current.sort,
              mode: current.mode,
              offset: 0,
              limit: Math.min(MAX_LIMIT, Math.max(PAGE, keep)),
              seed: current.seed,
            });
            outcome = { shown: { key, total: results.total, rows: results.rows } };
          }
        } catch (error) {
          outcome = { problem: message(error) };
        }
        if (!live.current) return;
        if (request !== wanted.current) continue;
        if ("shown" in outcome) {
          setShown(outcome.shown);
          setProblem(null);
        } else {
          setProblem(outcome.problem);
        }
        return;
      }
    } finally {
      window.clearTimeout(slow);
      running.current = false;
      if (live.current) setSearching(false);
    }
  }, [core]);

  const searchNow = useCallback(() => {
    window.clearTimeout(debounce.current);
    wanted.current += 1;
    void run();
  }, [run]);

  const update = useCallback(
    (change: (state: BrowseState) => BrowseState, typed = false) => {
      const next = change(stateRef.current);
      stateRef.current = next;
      setState(next);
      writeState(next);
      if (typed) {
        window.clearTimeout(debounce.current);
        debounce.current = window.setTimeout(searchNow, DELAY_MS);
      } else {
        searchNow();
      }
    },
    [searchNow],
  );

  const loadMore = useCallback(async () => {
    const before = shownRef.current;
    if (!before || before.rows.length >= before.total || running.current) return;
    const current = stateRef.current;
    if (searchKey(current) !== before.key) return;
    setLoadingMore(true);
    try {
      const results = await core.call("search", {
        query: toQuery(current.filters, current.text),
        sort: current.sort,
        mode: current.mode,
        offset: before.rows.length,
        limit: PAGE,
        seed: current.seed,
      });
      // Dropped if the search changed or a refresh replaced the rows meanwhile.
      if (live.current && shownRef.current === before) {
        const next = { ...before, total: results.total, rows: [...before.rows, ...results.rows] };
        shownRef.current = next;
        setShown(next);
      }
    } catch (error) {
      if (live.current) setProblem(message(error));
    } finally {
      if (live.current) setLoadingMore(false);
    }
  }, [core]);

  useEffect(() => {
    live.current = true;
    searchNow();
    const onVisible = () => {
      if (document.visibilityState === "visible") searchNow();
    };
    window.addEventListener("focus", searchNow);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      live.current = false;
      window.clearTimeout(debounce.current);
      window.removeEventListener("focus", searchNow);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [searchNow]);

  return {
    state,
    /** The rows on screen and how many the search found, or null before the first answer. */
    results: shown && { total: shown.total, rows: shown.rows },
    hasMore: shown !== null && shown.rows.length < shown.total,
    problem,
    searching,
    loadingMore,
    loadMore,
    /** Enter in the box: search now instead of waiting. */
    searchNow,
    setText: (text: string) => update((s) => ({ ...s, text }), true),
    /** Escape in the box: no wait, the text is gone. */
    clearText: () => update((s) => ({ ...s, text: "" })),
    setFilters: (filters: Filters) => update((s) => ({ ...s, filters })),
    setSort: (sort: SearchSort) =>
      update((s) => ({ ...s, sort, seed: sort.key === "random" ? newSeed() : s.seed })),
    setMode: (mode: SearchMode) => update((s) => ({ ...s, mode })),
    clearFilters: () => update((s) => ({ ...s, filters: noFilters })),
  };
}
