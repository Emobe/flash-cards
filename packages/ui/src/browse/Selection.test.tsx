import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type DeckList,
  type SearchInput,
  type SearchResults,
  type Target,
  type TargetSummary,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { editorHandlers } from "../editor/testEditor";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { BrowseScreen } from "./BrowseScreen";
import { resultRow } from "./testRows";
import { LONG_PRESS_MS } from "./useLongPress";
import { useSelection } from "./useSelection";

vi.mock("../editor/FieldEditor", () => import("../editor/testEditor"));

const deck = (id: string, name: string, path: string, depth: number) => ({
  id,
  name,
  path,
  parentId: null,
  depth,
  isDefault: id === "d-default",
  presetId: "p",
  newCount: 0,
  learningCount: 0,
  reviewCount: 0,
});

const decks: DeckList = {
  totalCards: 3,
  decks: [deck("d-default", "Default", "Default", 0), deck("d-polish", "Polish", "Polish", 0)],
};

function useWidth(wide: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: query.includes("min-width: 600px") ? wide : false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    })),
  );
}

const wait = (ms: number) => act(() => new Promise((resolve) => setTimeout(resolve, ms)));

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState(null, "", "/#/browse");
  useWidth(false);
});
afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await wait(10);
});

const threeRows: SearchResults = {
  total: 340,
  rows: [resultRow("kot"), resultRow("pies"), resultRow("ryba")],
};

function renderBrowse({
  search = () => threeRows,
  summary = (target: Target) => {
    const n = "notes" in target ? target.notes.length : target.cards.length;
    return { notes: n, cards: n, suspended: 0, noteCards: n };
  },
}: {
  search?: (input: SearchInput) => SearchResults;
  summary?: (target: Target) => TargetSummary;
} = {}) {
  const searchFn = vi.fn(search);
  let tokens = 0;
  const change = (message: string) => () => {
    tokens += 1;
    return { count: 1, undo: { id: `u-${tokens}`, message } };
  };
  const handlers = {
    searchIds: vi.fn((input: { query: string; mode: string }) => ({
      ids: Array.from({ length: 340 }, (_, i) => (input.mode === "notes" ? `n-${i}` : `c-${i}`)),
    })),
    describeTarget: vi.fn((input: { target: Target }) => summary(input.target)),
    moveCards: vi.fn(change("Moved 3 cards to Polish.")),
    addTags: vi.fn(change("Added verbs to 2 notes.")),
    removeTags: vi.fn(change("Removed verbs from 2 notes.")),
    suspendCards: vi.fn(change("Suspended 2 cards.")),
    unsuspendCards: vi.fn(change("Unsuspended 2 cards.")),
    deleteNotes: vi.fn(() => ({
      notes: 2,
      cards: 3,
      undo: { id: "u-delete", message: "Deleted 2 notes and their 3 cards." },
    })),
  };
  const edit = editorHandlers({ noteTypeId: "nt-1", fields: [] });
  const client = new CoreClient(
    createFakeTransport({
      ...edit,
      ...handlers,
      search: searchFn,
      checkQuery: () => ({ problem: null }),
      getDeckList: () => decks,
      getTags: () => ({ tags: ["verbs"] }),
      getNoteTypes: () => ({ noteTypes: [] }),
    }),
  );
  render(
    <CoreProvider client={client}>
      <RouterProvider>
        <AppShell>
          <BrowseScreen />
        </AppShell>
      </RouterProvider>
    </CoreProvider>,
  );
  return { searchFn, handlers, edit };
}

const row = (title: string) => screen.findByRole("button", { name: new RegExp(`^${title}`) });
const bar = () => screen.getByRole("toolbar", { name: "Selection" });
const checkbox = (title: string) => screen.getByLabelText(`Select ${title}`) as HTMLInputElement;
const lastInput = (fn: { mock: { lastCall?: unknown[] } }) => fn.mock.lastCall?.[0];

describe("selecting on a phone", () => {
  test("Select enters selection mode, a tap toggles a row, and ✕ leaves", async () => {
    const { edit } = renderBrowse();
    await row("kot");
    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    expect(within(bar()).getByText("Nothing selected")).not.toBeNull();
    expect(screen.queryByRole("searchbox", { name: "Search your cards" })).toBeNull();

    fireEvent.click(await row("kot"));
    fireEvent.click(await row("pies"));
    expect(checkbox("kot").checked).toBe(true);
    expect(within(bar()).getByText("2 notes")).not.toBeNull();
    fireEvent.click(await row("kot"));
    expect(checkbox("kot").checked).toBe(false);
    expect(edit.getNote).not.toHaveBeenCalled();

    fireEvent.click(within(bar()).getByRole("button", { name: "Leave selection" }));
    expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull();
    expect(screen.getByRole("searchbox", { name: "Search your cards" })).not.toBeNull();
    // Outside selection mode a tap opens the note again.
    fireEvent.click(await row("pies"));
    expect(await screen.findByRole("dialog", { name: "Edit note" })).not.toBeNull();
  });

  test("a long press selects the row without opening it; a short tap opens it", async () => {
    const { edit } = renderBrowse();
    const kot = await row("kot");
    fireEvent.pointerDown(kot, { button: 0, clientX: 10, clientY: 10 });
    await wait(LONG_PRESS_MS + 50);
    fireEvent.pointerUp(kot);
    fireEvent.click(kot);
    expect(checkbox("kot").checked).toBe(true);
    expect(within(bar()).getByText("1 note")).not.toBeNull();
    expect(screen.queryByRole("dialog", { name: "Edit note" })).toBeNull();
    expect(edit.getNote).not.toHaveBeenCalled();
  });

  test("moving the finger turns a long press into a scroll", async () => {
    renderBrowse();
    const kot = await row("kot");
    fireEvent.pointerDown(kot, { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(kot, { clientX: 10, clientY: 40 });
    await wait(LONG_PRESS_MS + 50);
    expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull();
  });

  test("the context menu and text selection are cancelled on rows", async () => {
    renderBrowse();
    const kot = await row("kot");
    expect(fireEvent.contextMenu(kot)).toBe(false);
    const select = new Event("selectstart", { bubbles: true, cancelable: true });
    kot.querySelector(".browse-title")?.dispatchEvent(select);
    expect(select.defaultPrevented).toBe(true);
  });

  test("Back leaves selection mode, and closes a sheet before that", async () => {
    renderBrowse();
    await row("kot");
    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    fireEvent.click(await row("kot"));
    fireEvent.click(within(bar()).getByRole("button", { name: "Move" }));
    expect(await screen.findByRole("dialog", { name: "Move to a deck" })).not.toBeNull();

    await act(async () => window.history.back());
    await wait(20);
    expect(screen.queryByRole("dialog", { name: "Move to a deck" })).toBeNull();
    expect(bar()).not.toBeNull();

    await act(async () => window.history.back());
    await wait(20);
    expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull();
    expect(window.location.hash).toBe("#/browse");
  });
});

describe("selecting from 600 px", () => {
  beforeEach(() => useWidth(true));

  test("a checkbox selects, the bar takes the panel's place, a row still opens its note", async () => {
    const { edit } = renderBrowse();
    await row("kot");
    expect(screen.queryByRole("button", { name: "Select" })).toBeNull();
    fireEvent.click(checkbox("kot"));
    expect(within(bar()).getByText("1 note")).not.toBeNull();
    expect(screen.queryByRole("searchbox", { name: "Search your cards" })).toBeNull();
    expect(within(bar()).getByRole("button", { name: "Add tags" })).not.toBeNull();
    expect(within(bar()).getByRole("button", { name: "Remove tags" })).not.toBeNull();
    expect(within(bar()).queryByRole("button", { name: "More" })).toBeNull();
    fireEvent.click(await row("pies"));
    await waitFor(() =>
      expect(edit.getNote).toHaveBeenCalledWith({ noteId: "n-pies" }, expect.anything()),
    );
  });

  test("Shift chooses a range of the loaded rows and Escape clears", async () => {
    renderBrowse();
    await row("kot");
    fireEvent.click(checkbox("kot"));
    fireEvent.click(checkbox("ryba"), { shiftKey: true });
    expect([checkbox("kot"), checkbox("pies"), checkbox("ryba")].map((c) => c.checked)).toEqual([
      true,
      true,
      true,
    ]);
    fireEvent.keyDown(checkbox("ryba"), { key: "Escape" });
    expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull();
    expect(checkbox("kot").checked).toBe(false);
  });

  test("Select all asks for every match and shows its count; so does Ctrl+A", async () => {
    const { handlers } = renderBrowse();
    await row("kot");
    fireEvent.click(checkbox("kot"));
    fireEvent.click(within(bar()).getByRole("button", { name: "Select all 340" }));
    expect(await within(bar()).findByText("340 notes")).not.toBeNull();
    expect(lastInput(handlers.searchIds)).toEqual({ query: "", mode: "notes" });
    expect(within(bar()).queryByRole("button", { name: /^Select all/ })).toBeNull();

    fireEvent.keyDown(document.body, { key: "Escape" });
    fireEvent.keyDown(document.body, { key: "a", ctrlKey: true });
    const selected = await screen.findByRole("toolbar", { name: "Selection" });
    expect(within(selected).getByText("340 notes")).not.toBeNull();
    expect(handlers.searchIds).toHaveBeenCalledTimes(2);
  });
});

describe("the actions", () => {
  beforeEach(() => useWidth(true));

  async function pick(...titles: string[]) {
    await row(titles[0] ?? "kot");
    for (const title of titles) fireEvent.click(checkbox(title));
  }

  test("Notes view counts the cards the notes have: 2 notes (3 cards)", async () => {
    renderBrowse({ summary: () => ({ notes: 2, cards: 3, suspended: 0, noteCards: 3 }) });
    await pick("kot", "pies");
    expect(await within(bar()).findByText("2 notes (3 cards)")).not.toBeNull();
  });

  test("Move sends the notes and the deck, shows the notice, and keeps the selection", async () => {
    const { handlers, searchFn } = renderBrowse();
    await pick("kot", "pies");
    fireEvent.click(within(bar()).getByRole("button", { name: "Move" }));
    const sheet = await screen.findByRole("dialog", { name: "Move to a deck" });
    expect(within(sheet).getByRole("button", { name: "Move" })).toHaveProperty("disabled", true);
    expect(within(sheet).queryByRole("button", { name: "Clear" })).toBeNull();
    fireEvent.click(within(sheet).getByLabelText("Polish"));
    const searches = searchFn.mock.calls.length;
    fireEvent.click(within(sheet).getByRole("button", { name: "Move" }));

    expect(await screen.findByText("Moved 3 cards to Polish.")).not.toBeNull();
    expect(lastInput(handlers.moveCards)).toEqual({
      target: { notes: ["n-kot", "n-pies"] },
      deckId: "d-polish",
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(within(bar()).getByText("2 notes")).not.toBeNull();
    expect(checkbox("kot").checked).toBe(true);
    await waitFor(() => expect(searchFn.mock.calls.length).toBeGreaterThan(searches));
  });

  test("Add tags and Remove tags send the typed tags to the notes", async () => {
    const { handlers } = renderBrowse();
    await pick("kot");
    fireEvent.click(within(bar()).getByRole("button", { name: "Add tags" }));
    let sheet = await screen.findByRole("dialog", { name: "Tags" });
    fireEvent.change(within(sheet).getByLabelText("Tags"), { target: { value: "verbs" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add tags" }));
    expect(await screen.findByText("Added verbs to 2 notes.")).not.toBeNull();
    expect(lastInput(handlers.addTags)).toEqual({ target: { notes: ["n-kot"] }, tags: ["verbs"] });

    fireEvent.click(within(bar()).getByRole("button", { name: "Remove tags" }));
    sheet = await screen.findByRole("dialog", { name: "Tags" });
    expect(within(sheet).getByRole("button", { name: "Remove" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    fireEvent.change(within(sheet).getByLabelText("Tags"), { target: { value: "verbs" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Remove tags" }));
    expect(await screen.findByText("Removed verbs from 2 notes.")).not.toBeNull();
    expect(handlers.removeTags).toHaveBeenCalledTimes(1);
  });

  test("Suspend reads Unsuspend when every selected card is suspended", async () => {
    let suspended = 0;
    const { handlers } = renderBrowse({
      summary: () => ({ notes: 2, cards: 2, suspended, noteCards: 2 }),
    });
    await pick("kot", "pies");
    fireEvent.click(await within(bar()).findByRole("button", { name: "Suspend" }));
    expect(await screen.findByText("Suspended 2 cards.")).not.toBeNull();
    expect(lastInput(handlers.suspendCards)).toEqual({ target: { notes: ["n-kot", "n-pies"] } });

    // The counts are read again after the change.
    suspended = 2;
    fireEvent.click(checkbox("ryba"));
    fireEvent.click(checkbox("ryba"));
    fireEvent.click(await within(bar()).findByRole("button", { name: "Unsuspend" }));
    expect(await screen.findByText("Unsuspended 2 cards.")).not.toBeNull();
    expect(handlers.unsuspendCards).toHaveBeenCalledTimes(1);
  });

  test("Delete asks with the counts, then clears the selection; Undo calls undoChange", async () => {
    const { handlers, edit } = renderBrowse({
      summary: () => ({ notes: 2, cards: 3, suspended: 0, noteCards: 3 }),
    });
    await pick("kot", "pies");
    await within(bar()).findByText("2 notes (3 cards)");
    fireEvent.click(within(bar()).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("dialog", {
      name: "Delete 2 notes and their 3 cards?",
    });
    expect(handlers.deleteNotes).not.toHaveBeenCalled();
    fireEvent.click(within(question).getByRole("button", { name: "Delete" }));

    const notice = await screen.findByText("Deleted 2 notes and their 3 cards.");
    expect(lastInput(handlers.deleteNotes)).toEqual({ target: { notes: ["n-kot", "n-pies"] } });
    expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull();
    expect(checkbox("kot").checked).toBe(false);

    const box = notice.closest(".undo-notice") as HTMLElement;
    fireEvent.click(within(box).getByRole("button", { name: "Undo" }));
    await waitFor(() =>
      expect(edit.undoChange).toHaveBeenCalledWith({ undoId: "u-delete" }, expect.anything()),
    );
  });

  test("in Cards view the target is cards and a delete counts every card of the notes", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ mode: "cards" }));
    const { handlers } = renderBrowse({
      summary: () => ({ notes: 1, cards: 1, suspended: 0, noteCards: 2 }),
    });
    await pick("kot");
    expect(within(bar()).getByText("1 card")).not.toBeNull();
    fireEvent.click(within(bar()).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("dialog", {
      name: "Delete 1 note and its 2 cards?",
    });
    fireEvent.click(within(question).getByRole("button", { name: "Cancel" }));
    fireEvent.click(within(bar()).getByRole("button", { name: "Suspend" }));
    await waitFor(() =>
      expect(lastInput(handlers.suspendCards)).toEqual({ target: { cards: ["c-kot"] } }),
    );
  });

  test("a refused action says why in the bar", async () => {
    const { handlers } = renderBrowse();
    handlers.suspendCards.mockImplementation(() => {
      throw { kind: "notFound", message: "That note or one of its cards was deleted." };
    });
    await pick("kot");
    fireEvent.click(within(bar()).getByRole("button", { name: "Suspend" }));
    expect((await within(bar()).findByRole("alert")).textContent).toBe(
      "That note or one of its cards was deleted.",
    );
  });
});

describe("useSelection", () => {
  test("a new scope clears the selection; leaving clears the mode too", () => {
    const { result, rerender } = renderHook(({ scope }) => useSelection(scope), {
      initialProps: { scope: "a" },
    });
    act(() => result.current.enter("n-1"));
    act(() => result.current.toggle("n-2"));
    expect(result.current.size).toBe(2);
    rerender({ scope: "b" });
    expect(result.current.size).toBe(0);
    expect(result.current.selecting).toBe(true);
    act(() => result.current.leave());
    expect(result.current.active).toBe(false);
  });

  test("a range runs from the last row toggled, either way", () => {
    const { result } = renderHook(() => useSelection("a"));
    const loaded = ["a", "b", "c", "d"];
    act(() => result.current.toggle("d"));
    act(() => result.current.range("b", loaded));
    expect([...result.current.ids].sort()).toEqual(["b", "c", "d"]);
  });
});
