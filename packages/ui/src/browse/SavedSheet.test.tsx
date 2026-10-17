import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type NewSavedSearchInput,
  type SavedSearchSummary,
  type SearchInput,
  type UpdateSavedSearchInput,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { BrowseScreen } from "./BrowseScreen";
import { resultRow } from "./testRows";

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState(null, "", "/#/browse");
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    })),
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const verbs: SavedSearchSummary = {
  id: "s-verbs",
  name: "Verbs to fix",
  query: 'deck:="Polish" is:new',
  sort: { key: "field", descending: false },
  mode: "cards",
};

/** A Browse screen over a fake core that keeps saved searches in a list, as the core does. */
function renderBrowse(start: SavedSearchSummary[] = [verbs]) {
  const store = start.map((s) => ({ ...s }));
  let next = 0;
  const taken = (name: string, id?: string) =>
    store.some((s) => s.id !== id && s.name.toLowerCase() === name.toLowerCase());
  const refuse = (message: string) => {
    throw { kind: "invalidInput", message };
  };
  const searchFn = vi.fn((_: SearchInput) => ({ total: 1, rows: [resultRow("kot")] }));
  const saveFn = vi.fn((input: NewSavedSearchInput) => {
    if (taken(input.name)) refuse(`There is already a saved search called ${input.name}.`);
    next += 1;
    const id = `s-new-${next}`;
    store.push({ id, ...input });
    return { id };
  });
  const updateFn = vi.fn((input: UpdateSavedSearchInput) => {
    const found = store.find((s) => s.id === input.id);
    if (!found) throw { kind: "notFound", message: "That saved search is gone." };
    if (input.name !== null && taken(input.name, input.id)) {
      refuse(`There is already a saved search called ${input.name}.`);
    }
    if (input.name !== null) found.name = input.name;
    if (input.query !== null) found.query = input.query;
    if (input.sort !== null) found.sort = input.sort;
    if (input.mode !== null) found.mode = input.mode;
    return null;
  });
  const deleteFn = vi.fn(({ id }: { id: string }) => {
    store.splice(
      store.findIndex((s) => s.id === id),
      1,
    );
    return null;
  });
  const client = new CoreClient(
    createFakeTransport({
      search: searchFn,
      checkQuery: () => ({ problem: null }),
      getDeckList: () => ({ totalCards: 1, decks: [] }),
      getTags: () => ({ tags: [] }),
      getNoteTypes: () => ({ noteTypes: [] }),
      getSavedSearches: () => ({
        searches: [...store].map((s) => ({ ...s })).sort((a, b) => a.name.localeCompare(b.name)),
      }),
      saveSearch: saveFn,
      updateSavedSearch: updateFn,
      deleteSavedSearch: deleteFn,
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
  return { store, searchFn, saveFn, updateFn, deleteFn };
}

async function openSheet() {
  fireEvent.click(await screen.findByRole("button", { name: "Saved" }));
  const sheet = await screen.findByRole("dialog", { name: "Saved searches" });
  await within(sheet)
    .findByRole("list", { name: "Saved searches" })
    .catch(() => null);
  return sheet;
}

function heading() {
  return document.querySelector(".browse-saved")?.textContent ?? null;
}

describe("Saved searches", () => {
  test("the Saved chip comes first in the row", async () => {
    renderBrowse();
    const row = await screen.findByRole("group", { name: "Filters" });
    expect(within(row).getAllByRole("button")[0]?.textContent).toBe("Saved");
  });

  test("tapping one loads its chips, sort and mode, and its name shows under the heading", async () => {
    const { searchFn } = renderBrowse();
    const sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Verbs to fix" }));
    await waitFor(() =>
      expect(searchFn).toHaveBeenLastCalledWith(
        expect.objectContaining({
          query: 'deck:="Polish" is:new',
          sort: { key: "field", descending: false },
          mode: "cards",
        }),
        expect.anything(),
      ),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(heading()).toBe("Saved: Verbs to fix");
    expect(screen.getByRole("button", { name: /^Deck: Polish/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^State: New/ })).toBeTruthy();
  });

  test("a saved query typed by hand opens with everything in the box", async () => {
    const typed = { ...verbs, id: "s-typed", name: "Typed", query: "deck:Polish kot or pies" };
    renderBrowse([typed]);
    const sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Typed" }));
    const box = (await screen.findByRole("searchbox", {
      name: "Search your cards",
    })) as HTMLInputElement;
    await waitFor(() => expect(box.value).toBe("deck:Polish kot or pies"));
    expect(screen.getByRole("button", { name: "Deck: any" })).toBeTruthy();
    expect(heading()).toBe("Saved: Typed");
  });

  test("Save names what is on screen and opens it", async () => {
    const { saveFn } = renderBrowse([]);
    const box = await screen.findByRole("searchbox", { name: "Search your cards" });
    fireEvent.change(box, { target: { value: "kot" } });
    const sheet = await openSheet();
    expect(within(sheet).getByText(/No saved searches yet/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Save this search" }));
    const name = await within(sheet).findByLabelText("Name");
    expect(document.activeElement).toBe(name);
    fireEvent.change(name, { target: { value: "Cats" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(saveFn.mock.calls[0]?.[0]).toEqual({
      name: "Cats",
      query: "(kot)",
      sort: { key: "created", descending: true },
      mode: "notes",
    });
    await waitFor(() => expect(heading()).toBe("Saved: Cats"));
  });

  test("a name in use is refused in the window, which stays open", async () => {
    renderBrowse();
    const sheet = await openSheet();
    fireEvent.click(within(sheet).getByRole("button", { name: "Save this search" }));
    fireEvent.change(await within(sheet).findByLabelText("Name"), {
      target: { value: "verbs to fix" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect((await within(sheet).findByRole("alert")).textContent).toBe(
      "There is already a saved search called verbs to fix.",
    );
    expect(within(sheet).getByLabelText<HTMLInputElement>("Name").value).toBe("verbs to fix");
  });

  test("a changed open search offers Update and Save as new; Update writes it", async () => {
    const { updateFn, store } = renderBrowse();
    let sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Verbs to fix" }));
    await waitFor(() => expect(heading()).toBe("Saved: Verbs to fix"));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search your cards" }), {
      target: { value: "być" },
    });
    expect(heading()).toBe("Saved: Verbs to fix · changed");
    sheet = await openSheet();
    expect(within(sheet).getByRole("button", { name: "Save as new" })).toBeTruthy();
    expect(within(sheet).queryByRole("button", { name: "Save this search" })).toBeNull();
    fireEvent.click(within(sheet).getByRole("button", { name: 'Update "Verbs to fix"' }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(updateFn.mock.calls[0]?.[0]).toEqual({
      id: "s-verbs",
      name: null,
      query: 'deck:="Polish" is:new (być)',
      sort: { key: "field", descending: false },
      mode: "cards",
    });
    expect(store[0]?.query).toBe('deck:="Polish" is:new (być)');
    await waitFor(() => expect(heading()).toBe("Saved: Verbs to fix"));
  });

  test("Rename changes the name, and a taken name is refused", async () => {
    const other = { ...verbs, id: "s-other", name: "Nouns" };
    const { store } = renderBrowse([verbs, other]);
    const sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Rename Verbs to fix" }));
    const name = await within(sheet).findByLabelText<HTMLInputElement>("Name");
    expect(name.value).toBe("Verbs to fix");
    fireEvent.change(name, { target: { value: "Nouns" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Rename" }));
    expect((await within(sheet).findByRole("alert")).textContent).toBe(
      "There is already a saved search called Nouns.",
    );
    fireEvent.change(name, { target: { value: "Verbs" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Rename" }));
    expect(await within(sheet).findByRole("button", { name: "Verbs" })).toBeTruthy();
    expect(store.find((s) => s.id === "s-verbs")?.name).toBe("Verbs");
  });

  test("Delete, then Undo saves it again with the same values and keeps it open", async () => {
    const { deleteFn, saveFn, store } = renderBrowse();
    let sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Verbs to fix" }));
    await waitFor(() => expect(heading()).toBe("Saved: Verbs to fix"));
    sheet = await openSheet();
    fireEvent.click(await within(sheet).findByRole("button", { name: "Delete Verbs to fix" }));
    expect(await within(sheet).findByText('Deleted "Verbs to fix".')).toBeTruthy();
    expect(deleteFn).toHaveBeenCalledWith({ id: "s-verbs" }, expect.anything());
    await waitFor(() => expect(heading()).toBeNull());
    fireEvent.click(within(sheet).getByRole("button", { name: "Undo" }));
    expect(await within(sheet).findByRole("button", { name: "Verbs to fix open" })).toBeTruthy();
    expect(saveFn.mock.calls[0]?.[0]).toEqual({
      name: verbs.name,
      query: verbs.query,
      sort: verbs.sort,
      mode: verbs.mode,
    });
    expect(store.map((s) => s.id)).toEqual(["s-new-1"]);
    await waitFor(() => expect(heading()).toBe("Saved: Verbs to fix"));
  });

  test("a remembered saved search that is gone is dropped quietly", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ savedId: "s-gone", text: "kot" }));
    const { searchFn } = renderBrowse();
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem("fc.browse") ?? "{}").savedId).toBeNull(),
    );
    expect(heading()).toBeNull();
    expect(searchFn.mock.lastCall?.[0].query).toBe("(kot)");
  });

  test("the × under the heading closes the saved search and keeps the search", async () => {
    localStorage.setItem("fc.browse", JSON.stringify({ savedId: "s-verbs", text: "kot" }));
    renderBrowse();
    fireEvent.click(await screen.findByRole("button", { name: "Close saved search Verbs to fix" }));
    expect(heading()).toBeNull();
    expect(
      screen.getByRole<HTMLInputElement>("searchbox", { name: "Search your cards" }).value,
    ).toBe("kot");
  });
});
