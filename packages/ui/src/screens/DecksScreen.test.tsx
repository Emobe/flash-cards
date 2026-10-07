import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, createFakeTransport, type DeckList, type DeckSummary } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { RouterProvider } from "../router";
import { DecksScreen } from "./DecksScreen";

function deck(
  id: string,
  name: string,
  depth: number,
  parentId: string | null,
  [newCount, learningCount, reviewCount]: [number, number, number] = [0, 0, 0],
): DeckSummary {
  return {
    id,
    name,
    path: name,
    parentId,
    depth,
    isDefault: id === "d-default",
    newCount,
    learningCount,
    reviewCount,
  };
}

const polish: DeckList = {
  totalCards: 9,
  decks: [
    deck("d-default", "Default", 0, null),
    deck("d-polish", "Polish", 0, null, [5, 1, 3]),
    deck("d-verbs", "Verbs", 1, "d-polish", [2, 1, 0]),
    deck("d-irregular", "Irregular", 2, "d-verbs", [1, 0, 0]),
    deck("d-words", "Words", 1, "d-polish", [3, 0, 3]),
  ],
};

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState(null, "", "/#/decks");
});
afterEach(cleanup);

function renderDecks(getDeckList: () => DeckList | Promise<DeckList>) {
  const handler = vi.fn(getDeckList);
  const client = new CoreClient(createFakeTransport({ getDeckList: handler }));
  render(
    <CoreProvider client={client}>
      <RouterProvider>
        <DecksScreen />
      </RouterProvider>
    </CoreProvider>,
  );
  return handler;
}

describe("Decks", () => {
  test("shows the tree with its counts, readable by a screen reader", async () => {
    renderDecks(() => polish);
    const row = (await screen.findByRole("link", { name: /^Polish/ })) as HTMLAnchorElement;
    expect(row.textContent).toBe("Polish5 new1 learning3 to review");
    expect(screen.getAllByRole("listitem").map((li) => li.textContent?.split(/\d/)[0])).toEqual([
      "Default",
      "Polish",
      "Verbs",
      "Irregular",
      "Words",
    ]);
    const verbs = screen.getByRole("link", { name: /^Verbs/ }).closest("li");
    expect(verbs?.getAttribute("style")).toContain("--depth: 1");
  });

  test("one tap on a deck opens its study screen", async () => {
    renderDecks(() => polish);
    const link = (await screen.findByRole("link", { name: /^Words/ })) as HTMLAnchorElement;
    expect(link.getAttribute("href")).toBe("#/study/d-words");
    fireEvent.click(link);
    expect(window.location.hash).toBe("#/study/d-words");
  });

  test("a deck with decks inside can be collapsed, and the choice is remembered", async () => {
    renderDecks(() => polish);
    const toggle = await screen.findByRole("button", { name: "Polish, decks inside" });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    // Decks with nothing inside have no toggle.
    expect(screen.queryByRole("button", { name: "Default, decks inside" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Verbs, decks inside" }));
    expect(screen.queryByRole("link", { name: /^Irregular/ })).toBeNull();
    expect(screen.getByRole("link", { name: /^Words/ })).toBeDefined();

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("link", { name: /^Verbs/ })).toBeNull();
    expect(screen.queryByRole("link", { name: /^Words/ })).toBeNull();
    expect(screen.getByRole("link", { name: /^Default/ })).toBeDefined();
    expect(JSON.parse(localStorage.getItem("fc.collapsedDecks") ?? "[]").sort()).toEqual([
      "d-polish",
      "d-verbs",
    ]);

    // Opening Polish again shows Verbs still closed.
    fireEvent.click(toggle);
    expect(screen.getByRole("link", { name: /^Verbs/ })).toBeDefined();
    expect(screen.queryByRole("link", { name: /^Irregular/ })).toBeNull();
  });

  test("decks collapsed earlier stay collapsed", async () => {
    localStorage.setItem("fc.collapsedDecks", JSON.stringify(["d-polish"]));
    renderDecks(() => polish);
    expect(await screen.findByRole("link", { name: /^Polish/ })).toBeDefined();
    expect(screen.queryByRole("link", { name: /^Verbs/ })).toBeNull();
  });

  test("a collection with no cards gets a plain welcome and a way to add one", async () => {
    renderDecks(() => ({ totalCards: 0, decks: [deck("d-default", "Default", 0, null)] }));
    expect(
      await screen.findByText("You have no cards yet. Add your first card to start studying."),
    ).toBeDefined();
    expect(screen.queryByRole("list")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add your first card" }));
    expect(window.location.hash).toBe("#/add");
  });

  test("when cards exist but nothing is due, it says so and still lists the decks", async () => {
    renderDecks(() => ({
      totalCards: 4,
      decks: [deck("d-default", "Default", 0, null), deck("d-polish", "Polish", 0, null)],
    }));
    expect(await screen.findByText("You're done for now. Nothing is due today.")).toBeDefined();
    expect(screen.getByRole("link", { name: /^Polish/ })).toBeDefined();
  });

  test("a failed load shows the problem and Try again loads once more", async () => {
    let fail = true;
    const handler = renderDecks(() => {
      if (fail) throw { kind: "internal", message: "Something went wrong inside the app." };
      return polish;
    });
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("Something went wrong inside the app.")).toBeDefined();
    fail = false;
    fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("link", { name: /^Polish/ })).toBeDefined();
    expect(handler).toHaveBeenCalledTimes(2);
  });

  test("the list is read again when the app comes back to the front", async () => {
    let list = polish;
    const handler = renderDecks(() => list);
    expect(await screen.findByRole("link", { name: /^Polish/ })).toBeDefined();

    list = { ...polish, decks: polish.decks.map((d) => ({ ...d, reviewCount: 7 })) };
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect((await screen.findByRole("link", { name: /^Default/ })).textContent).toBe(
      "Default0 new0 learning7 to review",
    );
    expect(handler).toHaveBeenCalledTimes(2);
  });

  test("a refresh that fails keeps the list on screen", async () => {
    let fail = false;
    renderDecks(() => {
      if (fail) throw { kind: "internal", message: "Something went wrong inside the app." };
      return polish;
    });
    expect(await screen.findByRole("link", { name: /^Polish/ })).toBeDefined();
    fail = true;
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(screen.getByRole("link", { name: /^Polish/ })).toBeDefined();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("Managing decks", () => {
  function renderManage(overrides: Record<string, (input: never) => unknown> = {}) {
    const calls: { method: string; input: unknown }[] = [];
    const record = (method: string, result: unknown) => (input: never) => {
      calls.push({ method, input });
      const custom = overrides[method];
      return custom ? custom(input) : result;
    };
    const client = new CoreClient(
      createFakeTransport({
        getDeckList: (input: never) =>
          overrides.getDeckList ? overrides.getDeckList(input) : polish,
        createDeck: record("createDeck", { id: "d-new" }),
        renameDeck: record("renameDeck", null),
        moveDeck: record("moveDeck", null),
        deleteDeck: record("deleteDeck", { decks: 3, cards: 9 }),
        restoreDeck: record("restoreDeck", null),
      } as never),
    );
    render(
      <CoreProvider client={client}>
        <RouterProvider>
          <DecksScreen />
        </RouterProvider>
      </CoreProvider>,
    );
    return calls;
  }

  async function manage() {
    fireEvent.click(await screen.findByRole("button", { name: "Manage" }));
  }

  test("Manage swaps the study links for actions, and Done swaps them back", async () => {
    renderManage();
    await manage();
    expect(screen.queryByRole("link", { name: /^Polish/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Rename Polish" })).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.getByRole("link", { name: /^Polish/ })).toBeDefined();
  });

  test("the Default deck has no Delete", async () => {
    renderManage();
    await manage();
    expect(screen.getByRole("button", { name: "Rename Default" })).toBeDefined();
    expect(screen.queryByRole("button", { name: "Delete Default" })).toBeNull();
    expect(screen.getByRole("button", { name: "Delete Polish" })).toBeDefined();
  });

  test("Add deck creates a deck inside the chosen parent and reads the list again", async () => {
    const calls = renderManage();
    fireEvent.click(await screen.findByRole("button", { name: "Add deck" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Czech" } });
    fireEvent.change(screen.getByLabelText("Inside"), { target: { value: "d-polish" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toEqual([
      { method: "createDeck", input: { name: "Czech", parentId: "d-polish" } },
    ]);
  });

  test("Add inside starts with that deck as the parent", async () => {
    renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Add inside Verbs" }));
    expect((screen.getByLabelText("Inside") as HTMLSelectElement).value).toBe("d-verbs");
  });

  test("Rename starts with the current name", async () => {
    const calls = renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Rename Words" }));
    const name = screen.getByLabelText("Name") as HTMLInputElement;
    expect(name.value).toBe("Words");
    fireEvent.change(name, { target: { value: "Nouns" } });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toEqual([{ method: "renameDeck", input: { deckId: "d-words", name: "Nouns" } }]);
  });

  test("Move does not offer the deck itself or the decks inside it", async () => {
    const calls = renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Move Verbs" }));
    const options = within(screen.getByLabelText("Move to")).getAllByRole("option");
    expect(options.map((o) => o.textContent?.trim())).toEqual([
      "Top level",
      "Default",
      "Polish",
      "Words",
    ]);
    expect((screen.getByLabelText("Move to") as HTMLSelectElement).value).toBe("d-polish");
    fireEvent.change(screen.getByLabelText("Move to"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toEqual([{ method: "moveDeck", input: { deckId: "d-verbs", parentId: null } }]);
  });

  test("a refused name shows the reason and keeps the window open with the text", async () => {
    renderManage({
      createDeck: () => {
        throw { kind: "invalidInput", message: 'There is already one called "Polish" here.' };
      },
    });
    fireEvent.click(await screen.findByRole("button", { name: "Add deck" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Polish" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect((await screen.findByRole("alert")).textContent).toContain("There is already one");
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Polish");
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(false);
  });

  test("Delete asks first, says what goes, and offers Undo afterwards", async () => {
    const calls = renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Delete Polish" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("and the 3 decks inside it");
    expect(calls).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    const notice = await screen.findByText('Deleted "Polish", with 3 decks inside and 9 cards.');
    expect(notice).toBeDefined();
    expect(calls).toEqual([{ method: "deleteDeck", input: { deckId: "d-polish" } }]);

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(screen.queryByText(/^Deleted "Polish"/)).toBeNull());
    expect(calls.at(-1)).toEqual({ method: "restoreDeck", input: { deckId: "d-polish" } });
  });

  test("the back button closes a window and stays on the Decks screen", async () => {
    renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Delete Polish" }));
    expect(screen.getByRole("dialog")).toBeDefined();
    act(() => window.history.back());
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(window.location.hash).toBe("#/decks");
    expect(screen.getByRole("button", { name: "Done" })).toBeDefined();
  });

  test("closing a window with a button leaves no extra history entry", async () => {
    renderManage();
    await manage();
    const before = window.history.length;
    fireEvent.click(screen.getByRole("button", { name: "Rename Polish" }));
    expect(window.history.state?.fcDialog).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(window.history.state?.fcDialog).toBeFalsy());
    expect(window.history.length).toBe(before + 1);
  });

  test("Cancel changes nothing", async () => {
    const calls = renderManage();
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Delete Polish" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls).toEqual([]);
  });

  test("a failed Undo says why and keeps the Undo button", async () => {
    renderManage({
      restoreDeck: () => {
        throw { kind: "invalidInput", message: 'There is already one called "Polish" here.' };
      },
    });
    await manage();
    fireEvent.click(screen.getByRole("button", { name: "Delete Polish" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    expect(await screen.findByText(/There is already one called/)).toBeDefined();
    expect(screen.getByRole("button", { name: "Undo" })).toBeDefined();
  });

  test("a collection with only the Default deck can still add a deck", async () => {
    renderManage({
      getDeckList: () => ({ totalCards: 0, decks: [deck("d-default", "Default", 0, null)] }),
    });
    expect(await screen.findByRole("button", { name: "Add deck" })).toBeDefined();
  });
});
