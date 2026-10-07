import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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
  return { id, name, path: name, parentId, depth, newCount, learningCount, reviewCount };
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
