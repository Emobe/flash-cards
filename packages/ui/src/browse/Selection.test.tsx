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
import { infoReview } from "../card/testInfo";
import { CoreProvider } from "../core";
import { editorHandlers } from "../editor/testEditor";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { BrowseScreen } from "./BrowseScreen";
import { resultRow } from "./testRows";
import { typeBasic, typeCloze, typeReversed } from "./testTypes";
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
    return { notes: n, cards: n, suspended: 0, noteCards: n, newCards: 0, noteTypeId: "nt-1" };
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
    setDueDate: vi.fn(() => ({
      count: 2,
      skippedNew: 1,
      undo: { id: "u-due", message: "Made 2 cards due tomorrow." },
    })),
    resetCards: vi.fn(change("Reset 1 card to new.")),
    getCardInfo: vi.fn((_input: { cardId: string }) => infoReview),
    changeNoteType: vi.fn(() => ({
      notes: 2,
      carried: 2,
      undo: { id: "u-type", message: "Changed 2 notes to Basic and reversed." },
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
      getNoteTypes: () => ({ noteTypes: [typeBasic, typeReversed, typeCloze] }),
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
    expect(within(bar()).getByRole("button", { name: "More" })).not.toBeNull();
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

  /** Set due date is in More, from 600 px too, with Reset to new and without Remove tags. */
  function openDue() {
    fireEvent.click(within(bar()).getByRole("button", { name: "More" }));
    const more = screen.getByRole("dialog", { name: "More actions" });
    expect(within(more).queryByRole("button", { name: "Remove tags" })).toBeNull();
    fireEvent.click(within(more).getByRole("button", { name: "Set due date" }));
  }

  test("Notes view counts the cards the notes have: 2 notes (3 cards)", async () => {
    renderBrowse({
      summary: () => ({
        notes: 2,
        cards: 3,
        suspended: 0,
        noteCards: 3,
        newCards: 0,
        noteTypeId: "nt-1",
      }),
    });
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
      summary: () => ({
        notes: 2,
        cards: 2,
        suspended,
        noteCards: 2,
        newCards: 0,
        noteTypeId: "nt-1",
      }),
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
      summary: () => ({
        notes: 2,
        cards: 3,
        suspended: 0,
        noteCards: 3,
        newCards: 0,
        noteTypeId: "nt-1",
      }),
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
      summary: () => ({
        notes: 1,
        cards: 1,
        suspended: 0,
        noteCards: 2,
        newCards: 0,
        noteTypeId: "nt-1",
      }),
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

  test("Set due date sends the chosen days, and says how many new cards it leaves", async () => {
    const { handlers, edit } = renderBrowse({
      summary: () => ({
        notes: 2,
        cards: 3,
        suspended: 0,
        noteCards: 3,
        newCards: 1,
        noteTypeId: "nt-1",
      }),
    });
    await pick("kot", "pies");
    await within(bar()).findByText("2 notes (3 cards)");
    openDue();
    let sheet = await screen.findByRole("dialog", { name: "Set due date" });
    expect(within(sheet).getByText("On 3 cards")).not.toBeNull();
    expect(within(sheet).getByText(/^1 new card is not changed/)).not.toBeNull();
    expect((within(sheet).getByLabelText("Tomorrow") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(within(sheet).getByRole("button", { name: "Set due date" }));

    const notice = await screen.findByText("Made 2 cards due tomorrow.");
    expect(lastInput(handlers.setDueDate)).toEqual({
      target: { notes: ["n-kot", "n-pies"] },
      fromDays: 1,
      toDays: 1,
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(checkbox("kot").checked).toBe(true);
    fireEvent.click(
      within(notice.closest(".undo-notice") as HTMLElement).getByRole("button", {
        name: "Undo",
      }),
    );
    await waitFor(() =>
      expect(edit.undoChange).toHaveBeenCalledWith({ undoId: "u-due" }, expect.anything()),
    );

    // Typing a number chooses its option.
    openDue();
    sheet = await screen.findByRole("dialog", { name: "Set due date" });
    fireEvent.change(within(sheet).getByLabelText("Number of days"), { target: { value: "10" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Set due date" }));
    await waitFor(() =>
      expect(lastInput(handlers.setDueDate)).toMatchObject({ fromDays: 10, toDays: 10 }),
    );

    openDue();
    sheet = await screen.findByRole("dialog", { name: "Set due date" });
    fireEvent.change(within(sheet).getByLabelText("Fewest days"), { target: { value: "9" } });
    fireEvent.change(within(sheet).getByLabelText("Most days"), { target: { value: "2" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Set due date" }));
    expect(within(sheet).getByRole("alert").textContent).toBe(
      "The first number of days cannot be more than the second.",
    );
    expect(handlers.setDueDate).toHaveBeenCalledTimes(2);
    fireEvent.change(within(sheet).getByLabelText("Most days"), { target: { value: "14" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Set due date" }));
    await waitFor(() =>
      expect(lastInput(handlers.setDueDate)).toMatchObject({ fromDays: 9, toDays: 14 }),
    );
  });

  /** Change note type waits for the selection's summary, which says if the notes share a type. */
  async function openChangeType() {
    fireEvent.click(within(bar()).getByRole("button", { name: "More" }));
    const more = screen.getByRole("dialog", { name: "More actions" });
    const button = within(more).getByRole("button", { name: "Change note type" });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(button);
  }

  test("Change note type sends the maps, keeps the selection and offers Undo", async () => {
    const { handlers, edit } = renderBrowse();
    await pick("kot", "pies");
    await within(bar()).findByText("2 notes");
    await openChangeType();
    const window = await screen.findByRole("dialog", { name: "Change note type" });
    fireEvent.click(await within(window).findByRole("button", { name: "Change" }));

    const notice = await screen.findByText("Changed 2 notes to Basic and reversed.");
    expect(lastInput(handlers.changeNoteType)).toMatchObject({
      target: { notes: ["n-kot", "n-pies"] },
      noteTypeId: "nt-2",
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(checkbox("kot").checked).toBe(true);
    fireEvent.click(
      within(notice.closest(".undo-notice") as HTMLElement).getByRole("button", { name: "Undo" }),
    );
    await waitFor(() =>
      expect(edit.undoChange).toHaveBeenCalledWith({ undoId: "u-type" }, expect.anything()),
    );
  });

  test("in Cards view the type change clears the selection, since the cards are replaced", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ mode: "cards" }));
    const { handlers } = renderBrowse();
    await pick("kot");
    await openChangeType();
    fireEvent.click(await screen.findByRole("button", { name: "Change" }));
    await waitFor(() => expect(handlers.changeNoteType).toHaveBeenCalled());
    expect(lastInput(handlers.changeNoteType)).toMatchObject({ target: { cards: ["c-kot"] } });
    await waitFor(() => expect(screen.queryByRole("toolbar", { name: "Selection" })).toBeNull());
  });

  test("a mixed selection is told to select one note type", async () => {
    renderBrowse({
      summary: () => ({
        notes: 2,
        cards: 3,
        suspended: 0,
        noteCards: 3,
        newCards: 0,
        noteTypeId: null,
      }),
    });
    await pick("kot", "pies");
    await within(bar()).findByText("2 notes (3 cards)");
    await openChangeType();
    expect(await screen.findByText("Select notes of one note type to change it.")).not.toBeNull();
  });

  test("Reset to new asks with the cards that are not new yet, then resets them", async () => {
    const { handlers } = renderBrowse({
      summary: () => ({
        notes: 2,
        cards: 3,
        suspended: 0,
        noteCards: 3,
        newCards: 2,
        noteTypeId: "nt-1",
      }),
    });
    await pick("kot", "pies");
    await within(bar()).findByText("2 notes (3 cards)");
    fireEvent.click(within(bar()).getByRole("button", { name: "More" }));
    fireEvent.click(
      within(await screen.findByRole("dialog", { name: "More actions" })).getByRole("button", {
        name: "Reset to new",
      }),
    );
    const question = await screen.findByRole("dialog", { name: "Reset 1 card to new?" });
    expect(within(question).getByText(/Their review history is kept/).textContent).toContain(
      "2 cards are new already and stay as they are.",
    );
    expect(handlers.resetCards).not.toHaveBeenCalled();
    fireEvent.click(within(question).getByRole("button", { name: "Reset" }));
    expect(await screen.findByText("Reset 1 card to new.")).not.toBeNull();
    expect(lastInput(handlers.resetCards)).toEqual({ target: { notes: ["n-kot", "n-pies"] } });
    expect(checkbox("kot").checked).toBe(true);
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

test("an undone notice stays undone when the layout changes", async () => {
  let wide = false;
  const listeners = new Set<() => void>();
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      get matches() {
        return query.includes("min-width: 600px") ? wide : false;
      },
      media: query,
      addEventListener: (_: string, f: () => void) => listeners.add(f),
      removeEventListener: (_: string, f: () => void) => listeners.delete(f),
    })),
  );
  const { edit } = renderBrowse();
  await row("kot");
  fireEvent.click(screen.getByRole("button", { name: "Select" }));
  fireEvent.click(await row("kot"));
  fireEvent.click(within(bar()).getByRole("button", { name: "Suspend" }));
  const notice = (await screen.findByText(/^Suspended/)).closest(".undo-notice") as HTMLElement;
  fireEvent.click(within(notice).getByRole("button", { name: "Undo" }));
  expect(await screen.findByText("Undone.")).not.toBeNull();

  wide = true;
  act(() => {
    for (const f of listeners) f();
  });
  const moved = screen.getByText("Undone.").closest(".undo-notice") as HTMLElement;
  expect(within(moved).queryByRole("button", { name: "Undo" })).toBeNull();
  expect(edit.undoChange).toHaveBeenCalledTimes(1);
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

test("under 600 px More has Card info, Set due date, Reset to new, Change note type and Remove tags", async () => {
  const { handlers } = renderBrowse();
  fireEvent.click(await screen.findByRole("button", { name: "Select" }));
  fireEvent.click(await row("kot"));
  fireEvent.click(within(bar()).getByRole("button", { name: "More" }));
  const more = await screen.findByRole("dialog", { name: "More actions" });
  const items = within(more)
    .getAllByRole("button")
    .map((b) => b.textContent);
  expect(items).toEqual([
    "Card info",
    "Set due date",
    "Reset to new",
    "Change note type",
    "Remove tags",
    "Cancel",
  ]);
  const actions = bar().querySelector(".selection-actions") as HTMLElement;
  expect(within(actions).queryByRole("button", { name: "Set due date" })).toBeNull();
  fireEvent.click(within(more).getByRole("button", { name: "Set due date" }));
  const sheet = await screen.findByRole("dialog", { name: "Set due date" });
  fireEvent.click(within(sheet).getByLabelText("Today"));
  fireEvent.click(within(sheet).getByRole("button", { name: "Set due date" }));
  await waitFor(() =>
    expect(lastInput(handlers.setDueDate)).toEqual({
      target: { notes: ["n-kot"] },
      fromDays: 0,
      toDays: 0,
    }),
  );
});

describe("card info from the selection (step 3.3)", () => {
  beforeEach(() => useWidth(true));

  const openMore = () => {
    fireEvent.click(within(bar()).getByRole("button", { name: "More" }));
    return screen.getByRole("dialog", { name: "More actions" });
  };

  test("with one row selected, More has Card info for that row's card", async () => {
    const { handlers } = renderBrowse();
    await row("kot");
    fireEvent.click(checkbox("kot"));
    fireEvent.click(within(openMore()).getByRole("button", { name: "Card info" }));
    expect(await screen.findByRole("dialog", { name: "Card info" })).not.toBeNull();
    expect(handlers.getCardInfo).toHaveBeenCalledWith({ cardId: "c-kot" }, expect.anything());
    // Closing it leaves the selection as it was.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Card info" })).toBeNull());
    expect(checkbox("kot").checked).toBe(true);
  });

  test("with several rows selected, Card info is off", async () => {
    renderBrowse();
    await row("kot");
    fireEvent.click(checkbox("kot"));
    fireEvent.click(checkbox("pies"));
    const more = openMore();
    expect(
      (within(more).getByRole("button", { name: "Card info" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});
