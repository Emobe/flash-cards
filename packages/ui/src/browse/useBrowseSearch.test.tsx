import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { CoreClient, createFakeTransport, type SearchInput, type SearchResults } from "core-client";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { noFilters } from "./query";
import { resultRow } from "./testRows";
import { DELAY_MS, defaultState, differsFrom, readState, useBrowseSearch } from "./useBrowseSearch";

function results(total: number, from = 0, count = Math.min(total - from, 100)): SearchResults {
  return { total, rows: Array.from({ length: count }, (_, i) => resultRow(`r${from + i}`)) };
}

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function setup(
  search: (input: SearchInput) => SearchResults | Promise<SearchResults>,
  checkQuery: (input: { query: string }) => {
    problem: { message: string; position: number } | null;
  } = () => ({ problem: null }),
) {
  const searchFn = vi.fn(search);
  const checkFn = vi.fn(checkQuery);
  const client = new CoreClient(createFakeTransport({ search: searchFn, checkQuery: checkFn }));
  const wrapper = ({ children }: { children: ReactNode }) => (
    <CoreProvider client={client}>{children}</CoreProvider>
  );
  const hook = renderHook(() => useBrowseSearch(), { wrapper });
  return { hook, searchFn, checkFn };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("useBrowseSearch", () => {
  test("searches on mount with the default state", async () => {
    const { hook, searchFn } = setup(() => results(3));
    await waitFor(() => expect(hook.result.current.results?.total).toBe(3));
    expect(searchFn).toHaveBeenCalledTimes(1);
    expect(searchFn.mock.calls[0]?.[0]).toEqual({
      query: "",
      sort: { key: "created", descending: true },
      mode: "notes",
      offset: 0,
      limit: 100,
      seed: 0,
    });
  });

  test("typing waits for a pause, then searches once with the text in brackets", async () => {
    const { hook, searchFn, checkFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    act(() => hook.result.current.setText("k"));
    act(() => hook.result.current.setText("ko"));
    act(() => hook.result.current.setText("kot"));
    await sleep(DELAY_MS / 2);
    expect(searchFn).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2));
    await sleep(DELAY_MS);
    expect(searchFn).toHaveBeenCalledTimes(2);
    expect(searchFn.mock.calls[1]?.[0].query).toBe("(kot)");
    expect(checkFn).toHaveBeenCalledWith({ query: "kot" }, expect.anything());
  });

  test("Enter searches at once", async () => {
    const { hook, searchFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    act(() => hook.result.current.setText("kot"));
    act(() => hook.result.current.searchNow());
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2));
  });

  test("a chip, the sort and the mode search at once and reach the call", async () => {
    const { hook, searchFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    act(() => hook.result.current.setFilters({ ...noFilters, decks: ["Polish"] }));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2));
    expect(searchFn.mock.calls[1]?.[0].query).toBe('deck:="Polish"');
    act(() => hook.result.current.setSort({ key: "due", descending: false }));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(3));
    expect(searchFn.mock.calls[2]?.[0].sort).toEqual({ key: "due", descending: false });
    act(() => hook.result.current.setMode("cards"));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(4));
    expect(searchFn.mock.calls[3]?.[0].mode).toBe("cards");
  });

  test("Shuffle takes a new seed each time it is chosen", async () => {
    const { hook, searchFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    const seeds = new Set<number>();
    for (let i = 0; i < 3; i += 1) {
      act(() => hook.result.current.setSort({ key: "random", descending: false }));
      await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2 + i));
      seeds.add(searchFn.mock.calls[1 + i]?.[0].seed ?? -1);
    }
    expect(seeds.size).toBe(3);
  });

  test("one search at a time, and a result for an older state is dropped", async () => {
    const pending: { input: SearchInput; resolve: (r: SearchResults) => void }[] = [];
    const { hook, searchFn } = setup(
      (input) => new Promise((resolve) => pending.push({ input, resolve })),
    );
    await waitFor(() => expect(pending).toHaveLength(1));
    act(() => hook.result.current.setFilters({ ...noFilters, states: ["new"] }));
    act(() => hook.result.current.setFilters({ ...noFilters, states: ["review"] }));
    await sleep(20);
    expect(searchFn).toHaveBeenCalledTimes(1);
    await act(async () => pending[0]?.resolve(results(5)));
    await waitFor(() => expect(pending).toHaveLength(2));
    expect(hook.result.current.results).toBeNull();
    expect(pending[1]?.input.query).toBe("is:review");
    await act(async () => pending[1]?.resolve(results(2)));
    await waitFor(() => expect(hook.result.current.results?.total).toBe(2));
    expect(searchFn).toHaveBeenCalledTimes(2);
  });

  test("a mistake in the box shows its message, keeps the rows and makes no search", async () => {
    const { hook, searchFn } = setup(
      () => results(4),
      ({ query }) => ({
        problem: query.includes('"') ? { message: "A quote is never closed.", position: 0 } : null,
      }),
    );
    await waitFor(() => expect(hook.result.current.results?.total).toBe(4));
    act(() => hook.result.current.setText('"kot'));
    act(() => hook.result.current.searchNow());
    await waitFor(() => expect(hook.result.current.problem).toBe("A quote is never closed."));
    expect(searchFn).toHaveBeenCalledTimes(1);
    expect(hook.result.current.results?.total).toBe(4);
    act(() => hook.result.current.clearText());
    await waitFor(() => expect(hook.result.current.problem).toBeNull());
    expect(hook.result.current.state.text).toBe("");
  });

  test("a search the core refuses shows the core's sentence", async () => {
    const { hook } = setup(() => {
      throw { kind: "invalidInput", message: "There is no field called nope." };
    });
    await waitFor(() => expect(hook.result.current.problem).toBe("There is no field called nope."));
  });

  test("paging asks for the next 100 and appends them", async () => {
    const { hook, searchFn } = setup((input) => results(250, input.offset));
    await waitFor(() => expect(hook.result.current.results?.rows).toHaveLength(100));
    expect(hook.result.current.hasMore).toBe(true);
    await act(() => hook.result.current.loadMore());
    expect(searchFn.mock.calls[1]?.[0]).toMatchObject({ offset: 100, limit: 100 });
    expect(hook.result.current.results?.rows).toHaveLength(200);
    expect(hook.result.current.results?.rows[150]?.title).toBe("r150");
    await act(() => hook.result.current.loadMore());
    expect(hook.result.current.results?.rows).toHaveLength(250);
    expect(hook.result.current.hasMore).toBe(false);
  });

  test("coming back to the window refreshes and keeps the rows already loaded", async () => {
    const { hook, searchFn } = setup((input) => results(250, input.offset, input.limit));
    await waitFor(() => expect(hook.result.current.results?.rows).toHaveLength(100));
    await act(() => hook.result.current.loadMore());
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(3));
    expect(searchFn.mock.calls[2]?.[0]).toMatchObject({ offset: 0, limit: 200 });
  });

  test("the search is remembered and restored", async () => {
    const first = setup(() => results(1));
    await waitFor(() => expect(first.searchFn).toHaveBeenCalledTimes(1));
    act(() => first.hook.result.current.setFilters({ ...noFilters, tags: ["verbs"] }));
    act(() => first.hook.result.current.setText("kot"));
    act(() => first.hook.result.current.setMode("cards"));
    first.hook.unmount();
    const second = setup(() => results(1));
    await waitFor(() => expect(second.searchFn).toHaveBeenCalledTimes(1));
    expect(second.searchFn.mock.calls[0]?.[0]).toMatchObject({
      query: 'tag:="verbs" (kot)',
      mode: "cards",
    });
  });

  test("opening a saved search loads its chips, text, sort and mode and searches at once", async () => {
    const { hook, searchFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    act(() => hook.result.current.setText("old"));
    act(() =>
      hook.result.current.openSaved({
        id: "s1",
        name: "Verbs",
        query: 'deck:="Polish" is:new (kot)',
        sort: { key: "field", descending: false },
        mode: "cards",
      }),
    );
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2));
    expect(searchFn.mock.calls[1]?.[0]).toMatchObject({
      query: 'deck:="Polish" is:new (kot)',
      sort: { key: "field", descending: false },
      mode: "cards",
    });
    expect(hook.result.current.state).toMatchObject({
      filters: { ...noFilters, decks: ["Polish"], states: ["new"] },
      text: "kot",
      savedId: "s1",
    });
  });

  test("a saved query typed by hand opens with everything in the box", async () => {
    const { hook, searchFn } = setup(() => results(1));
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(1));
    act(() =>
      hook.result.current.openSaved({
        id: "s2",
        name: "Typed",
        query: "deck:Polish kot or pies",
        sort: defaultState.sort,
        mode: "notes",
      }),
    );
    await waitFor(() => expect(searchFn).toHaveBeenCalledTimes(2));
    expect(hook.result.current.state.filters).toEqual(noFilters);
    expect(hook.result.current.state.text).toBe("deck:Polish kot or pies");
    expect(searchFn.mock.calls[1]?.[0].query).toBe("(deck:Polish kot or pies)");
  });
});

describe("differsFrom", () => {
  const saved = {
    id: "s1",
    name: "Typed",
    query: "kot",
    sort: { key: "random" as const, descending: false },
    mode: "notes" as const,
  };
  const state = { ...defaultState, text: "kot", sort: saved.sort, seed: 99 };

  test("a hand-typed query is the same once in brackets, and the seed does not count", () => {
    expect(differsFrom(state, saved)).toBe(false);
  });

  test("a chip, the text, the sort or the mode makes it changed", () => {
    expect(differsFrom({ ...state, filters: { ...noFilters, due: "now" } }, saved)).toBe(true);
    expect(differsFrom({ ...state, text: "pies" }, saved)).toBe(true);
    expect(differsFrom({ ...state, sort: { key: "random", descending: true } }, saved)).toBe(true);
    expect(differsFrom({ ...state, mode: "cards" }, saved)).toBe(true);
  });
});

describe("readState", () => {
  test("a broken stored value reads as the default", () => {
    for (const stored of ["{", "null", "[]", "42", '"x"']) {
      localStorage.setItem("fc.browse", stored);
      expect(readState(), stored).toEqual(defaultState);
    }
  });

  test("broken parts fall back one by one", () => {
    localStorage.setItem(
      "fc.browse",
      JSON.stringify({
        filters: { decks: ["Polish", 3], tags: ["x"], states: ["new", "gone"], due: "never" },
        text: 7,
        sort: { key: "nope", descending: true },
        mode: "cards",
        seed: -1,
        savedId: 5,
      }),
    );
    expect(readState()).toEqual({
      ...defaultState,
      filters: { ...noFilters, tags: ["x"], states: ["new"] },
      mode: "cards",
    });
  });

  test("the open saved search is remembered by its ID", () => {
    localStorage.setItem("fc.browse", JSON.stringify({ savedId: "s1" }));
    expect(readState().savedId).toBe("s1");
  });
});
