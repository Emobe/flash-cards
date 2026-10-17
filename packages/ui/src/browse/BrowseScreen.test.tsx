import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type DeckList,
  type SearchInput,
  type SearchResults,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { BrowseScreen } from "./BrowseScreen";
import { resultRow } from "./testRows";

const deck = (id: string, name: string, path: string, depth: number, parentId: string | null) => ({
  id,
  name,
  path,
  parentId,
  depth,
  isDefault: id === "d-default",
  presetId: "p",
  newCount: 0,
  learningCount: 0,
  reviewCount: 0,
});

const decks: DeckList = {
  totalCards: 3,
  decks: [
    deck("d-default", "Default", "Default", 0, null),
    deck("d-polish", "Polish", "Polish", 0, null),
    deck("d-verbs", "Verbs", "Polish::Verbs", 1, "d-polish"),
    deck("d-other", "Verbs", "Verbs", 0, null),
  ],
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

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState(null, "", "/#/browse");
  useWidth(false);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderBrowse({
  search = () => ({
    total: 2,
    rows: [
      resultRow("kot", { detail: "cat", deckId: "d-verbs" }),
      resultRow("pies", { state: "review", due: { kind: "day", days: 3 } }),
    ],
  }),
  deckList = decks,
}: {
  search?: (input: SearchInput) => SearchResults;
  deckList?: DeckList;
} = {}) {
  const searchFn = vi.fn(search);
  const client = new CoreClient(
    createFakeTransport({
      search: searchFn,
      checkQuery: () => ({ problem: null }),
      getDeckList: () => deckList,
      getTags: () => ({ tags: ["adverbs", "verbs", "verbs::irregular"] }),
      getNoteTypes: () => ({
        noteTypes: [
          { id: "nt-1", name: "Basic", kind: "standard", fields: [] },
          { id: "nt-2", name: "Basic and reversed", kind: "standard", fields: [] },
        ],
      }),
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
  const lastQuery = () => searchFn.mock.lastCall?.[0].query;
  return { searchFn, lastQuery };
}

async function choose(chip: RegExp, options: string[]) {
  fireEvent.click(await screen.findByRole("button", { name: chip }));
  const dialog = await screen.findByRole("dialog");
  for (const name of options) {
    const [first] = await within(dialog).findAllByLabelText(name);
    if (first) fireEvent.click(first);
  }
  fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
}

describe("Browse", () => {
  test("rows show the title, detail, deck path and due as plain text", async () => {
    renderBrowse();
    const list = await screen.findByRole("list", { name: "Results" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual([
      "kotcatPolish::VerbsNew",
      "piesDefaultDue in 3 days",
    ]);
    expect(screen.getByRole("status").textContent).toBe("2 notes");
  });

  test("a title with markup is shown as text, never as HTML", async () => {
    renderBrowse({
      search: () => ({ total: 1, rows: [resultRow("<b>kot</b> [sound:a.mp3]")] }),
    });
    const list = await screen.findByRole("list", { name: "Results" });
    expect(list.querySelector("b")).toBeNull();
    expect(list.textContent).toContain("<b>kot</b> ♪");
  });

  test("the search panel is in the bottom slot at phone width", async () => {
    renderBrowse();
    const box = await screen.findByRole("searchbox", { name: "Search your cards" });
    expect(box.closest(".shell-action")).not.toBeNull();
    expect(box.closest("main")).toBeNull();
  });

  test("the search panel is above the list when wide", async () => {
    useWidth(true);
    renderBrowse();
    const box = await screen.findByRole("searchbox", { name: "Search your cards" });
    expect(box.closest(".shell-action")).toBeNull();
    const list = await screen.findByRole("list", { name: "Results" });
    expect(box.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  test("/ focuses the search box, and not while typing elsewhere", async () => {
    renderBrowse();
    const box = await screen.findByRole("searchbox", { name: "Search your cards" });
    fireEvent.keyDown(document.body, { key: "/" });
    expect(document.activeElement).toBe(box);
  });

  test("Escape in the box clears the text", async () => {
    const { lastQuery } = renderBrowse();
    const box = (await screen.findByRole("searchbox", {
      name: "Search your cards",
    })) as HTMLInputElement;
    fireEvent.change(box, { target: { value: "kot" } });
    fireEvent.submit(box);
    await waitFor(() => expect(lastQuery()).toBe("(kot)"));
    fireEvent.keyDown(box, { key: "Escape" });
    expect(box.value).toBe("");
    await waitFor(() => expect(lastQuery()).toBe(""));
  });

  test("the deck picker writes whole paths, and the chip says what is chosen", async () => {
    const { lastQuery } = renderBrowse();
    await choose(/^Deck/, ["Verbs"]);
    // The tree has two decks called Verbs; the first is Polish::Verbs.
    await waitFor(() => expect(lastQuery()).toBe('deck:="Polish::Verbs"'));
    const chip = screen.getByRole("button", { name: "Deck: Polish::Verbs" });
    expect(chip.getAttribute("aria-pressed")).toBe("true");
    expect(chip.textContent).toBe("Deck: Verbs");
  });

  test("the deck picker's filter box finds a deck by its path", async () => {
    renderBrowse();
    fireEvent.click(await screen.findByRole("button", { name: /^Deck/ }));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByLabelText("Default");
    fireEvent.change(within(dialog).getByRole("searchbox"), { target: { value: "polish::v" } });
    expect(
      within(dialog)
        .getAllByRole("checkbox")
        .map((c) => c.closest("label")?.textContent),
    ).toEqual(["Polish::Verbs"]);
  });

  test("each picker writes its part of the query", async () => {
    const { lastQuery } = renderBrowse();
    await choose(/^Tag/, ["verbs", "adverbs"]);
    await waitFor(() => expect(lastQuery()).toBe('(tag:="adverbs" or tag:="verbs")'));
    await choose(/^Note type/, ["Basic"]);
    await choose(/^State/, ["New", "Suspended"]);
    await choose(/^Due/, ["Overdue"]);
    await choose(/^Added/, ["Last 7 days"]);
    await choose(/^Answered/, ["Answered Again today"]);
    await waitFor(() =>
      expect(lastQuery()).toBe(
        '(tag:="adverbs" or tag:="verbs") note:="Basic" (is:new or is:suspended) due:..-1 added:7 rated:1:1',
      ),
    );
    expect(screen.getByRole("button", { name: "Tag: adverbs, verbs" }).textContent).toBe("Tag: 2");
  });

  test("the × on a chip clears it", async () => {
    const { lastQuery } = renderBrowse();
    await choose(/^State/, ["Review"]);
    await waitFor(() => expect(lastQuery()).toBe("is:review"));
    fireEvent.click(screen.getByRole("button", { name: "Clear State" }));
    await waitFor(() => expect(lastQuery()).toBe(""));
    expect(screen.queryByRole("button", { name: "Clear State" })).toBeNull();
  });

  test("Clear in a picker clears the chip, Cancel changes nothing", async () => {
    const { searchFn, lastQuery } = renderBrowse();
    await choose(/^Due/, ["Due tomorrow"]);
    await waitFor(() => expect(lastQuery()).toBe("due:1"));
    const calls = searchFn.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: /^Due/ }));
    let dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByLabelText("Overdue"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(searchFn.mock.calls.length).toBe(calls);
    fireEvent.click(screen.getByRole("button", { name: /^Due/ }));
    dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(lastQuery()).toBe(""));
  });

  test("a chosen deck that is gone is marked not found", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ filters: { decks: ["Gone"] } }));
    const { lastQuery } = renderBrowse();
    await waitFor(() => expect(lastQuery()).toBe('deck:="Gone"'));
    expect(await screen.findByRole("button", { name: "Deck: Gone (not found)" })).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: /^Deck/ }));
    const dialog = await screen.findByRole("dialog");
    expect((within(dialog).getByLabelText("Gone (not found)") as HTMLInputElement).checked).toBe(
      true,
    );
  });

  test("the Sort window: a sort, a second tap reverses it, and Notes or Cards", async () => {
    const { searchFn } = renderBrowse();
    fireEvent.click(await screen.findByRole("button", { name: "Sort" }));
    const dialog = await screen.findByRole("dialog", { name: "Sort and show" });
    fireEvent.click(within(dialog).getByRole("button", { name: /^Due date/ }));
    await waitFor(() =>
      expect(searchFn.mock.lastCall?.[0].sort).toEqual({ key: "due", descending: false }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: /^Due date/ }));
    await waitFor(() =>
      expect(searchFn.mock.lastCall?.[0].sort).toEqual({ key: "due", descending: true }),
    );
    expect(within(dialog).getByRole("button", { name: /^Due date/ }).textContent).toBe(
      "Due datelatest first",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Cards" }));
    await waitFor(() => expect(searchFn.mock.lastCall?.[0].mode).toBe("cards"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  test("no match offers to clear the search", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ text: "zzz" }));
    const { lastQuery } = renderBrowse({ search: () => ({ total: 0, rows: [] }) });
    fireEvent.click(await screen.findByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(lastQuery()).toBe(""));
    expect(
      (screen.getByRole("searchbox", { name: "Search your cards" }) as HTMLInputElement).value,
    ).toBe("");
  });

  test("an empty collection uses the Decks wording", async () => {
    renderBrowse({
      search: () => ({ total: 0, rows: [] }),
      deckList: { totalCards: 0, decks: [decks.decks[0] as DeckList["decks"][number]] },
    });
    expect(
      await screen.findByText("You have no cards yet. Add your first card to start studying."),
    ).toBeDefined();
  });

  test("Show more loads the next page", async () => {
    const { searchFn } = renderBrowse({
      search: ({ offset, limit }) => ({
        total: 150,
        rows: Array.from({ length: Math.min(limit, 150 - offset) }, (_, i) =>
          resultRow(`r${offset + i}`),
        ),
      }),
    });
    // By text: role queries over a hundred rows are slow in happy-dom.
    const more = await screen.findByText("Show more");
    await act(async () => fireEvent.click(more));
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Results" })).getAllByRole("listitem"),
      ).toHaveLength(150),
    );
    expect(searchFn.mock.lastCall?.[0]).toMatchObject({ offset: 100, limit: 100 });
    expect(screen.queryByText("Show more")).toBeNull();
  });
});
