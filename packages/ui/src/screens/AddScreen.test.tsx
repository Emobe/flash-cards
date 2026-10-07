import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CoreClient, createFakeTransport, type NoteTypeSummary } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { AddScreen } from "./AddScreen";

// A real ProseMirror view cannot be typed into under happy-dom, so the screen is tested with a
// text area in its place. The editor itself is covered by html.test.ts and commands.test.ts, and
// by a run in a real browser (ADR 0011 build notes).
const focusSpies = new Map<string, ReturnType<typeof vi.fn>>();
vi.mock("../editor/FieldEditor", async () => {
  const { useEffect } = await import("react");
  return {
    FieldEditor: ({
      value,
      onChange,
      labelId,
      onReady,
    }: {
      value: string;
      onChange: (html: string) => void;
      labelId: string;
      onReady?: (view: unknown) => void;
    }) => {
      // Once per mount, like the real editor (its `onReady` is read through a ref).
      useEffect(() => {
        const focus = vi.fn();
        focusSpies.set(labelId, focus);
        onReady?.({ focus });
        return () => onReady?.(null);
      }, [labelId]);
      return (
        <textarea
          aria-labelledby={labelId}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      );
    },
  };
});

const noteTypes: NoteTypeSummary[] = [
  {
    id: "t-basic",
    name: "Basic",
    kind: "standard",
    fields: [
      { id: "f-front", name: "Front" },
      { id: "f-back", name: "Back" },
    ],
  },
  {
    id: "t-cloze",
    name: "Cloze",
    kind: "cloze",
    fields: [
      { id: "f-text", name: "Text" },
      { id: "f-extra", name: "Extra" },
    ],
  },
  {
    id: "t-word",
    name: "Word",
    kind: "standard",
    fields: [
      { id: "f-w-front", name: "Front" },
      { id: "f-w-notes", name: "Notes" },
    ],
  },
];

const deckSummary = (id: string, name: string, path: string, depth = 0) => ({
  id,
  name,
  path,
  parentId: null,
  depth,
  newCount: 0,
  learningCount: 0,
  reviewCount: 0,
});

const decks = [
  deckSummary("d-default", "Default", "Default"),
  deckSummary("d-polish", "Polish", "Polish"),
  deckSummary("d-verbs", "Verbs", "Polish::Verbs", 1),
];

type AddInput = {
  deckId: string;
  noteTypeId: string;
  fields: { fieldId: string; value: string }[];
  tags: string[];
};

function setup(
  overrides: {
    addNote?: (input: AddInput) => unknown;
    findDuplicates?: (input: { noteTypeId: string; value: string }) => string[];
    getDeckList?: () => unknown;
  } = {},
) {
  const handlers = {
    getDeckList: vi.fn(overrides.getDeckList ?? (() => ({ decks, totalCards: 0 }))),
    getNoteTypes: vi.fn(() => ({ noteTypes })),
    getTags: vi.fn(() => ({ tags: ["animals", "Anatomy", "pl", "pl::verbs"] })),
    findDuplicates: vi.fn((input: { noteTypeId: string; value: string }) => ({
      noteIds: overrides.findDuplicates?.(input) ?? [],
    })),
    addNote: vi.fn(
      (input: AddInput) =>
        (overrides.addNote?.(input) ?? { noteId: "n1", cardCount: 1, duplicates: [] }) as {
          noteId: string;
          cardCount: number;
          duplicates: string[];
        },
    ),
  };
  const client = new CoreClient(createFakeTransport(handlers as never));
  const view = render(
    <CoreProvider client={client}>
      <RouterProvider>
        <AppShell>
          <AddScreen />
        </AppShell>
      </RouterProvider>
    </CoreProvider>,
  );
  return { ...view, ...handlers };
}

const field = (name: string) => screen.getByRole("textbox", { name }) as HTMLTextAreaElement;
const type = (name: string, value: string) => fireEvent.change(field(name), { target: { value } });
const addButton = () => screen.getByRole("button", { name: "Add" }) as HTMLButtonElement;

async function ready() {
  await screen.findByLabelText("Deck");
}

beforeEach(() => {
  localStorage.clear();
  focusSpies.clear();
  window.history.replaceState(null, "", "/#/add");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Add", () => {
  test("starts on the Default deck and Basic with a box for each field", async () => {
    setup();
    await ready();
    expect((screen.getByLabelText("Deck") as HTMLSelectElement).value).toBe("d-default");
    expect((screen.getByLabelText("Type") as HTMLSelectElement).value).toBe("t-basic");
    expect(field("Front")).toBeTruthy();
    expect(field("Back")).toBeTruthy();
    expect(screen.getByRole("option", { name: "Polish::Verbs" })).toBeTruthy();
  });

  test("adds the note with its deck, fields and tags, then clears the fields and keeps the rest", async () => {
    const { addNote } = setup({
      addNote: () => ({ noteId: "n1", cardCount: 2, duplicates: [] }),
    });
    await ready();
    fireEvent.change(screen.getByLabelText("Deck"), { target: { value: "d-verbs" } });
    type("Front", "<b>kot</b>");
    type("Back", "cat");
    const tags = screen.getByLabelText("Tags");
    fireEvent.change(tags, { target: { value: "animals " } });
    fireEvent.change(tags, { target: { value: "pl" } });
    fireEvent.click(addButton());

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Added (2 cards)."));
    expect(addNote).toHaveBeenCalledWith(
      {
        deckId: "d-verbs",
        noteTypeId: "t-basic",
        fields: [
          { fieldId: "f-front", value: "<b>kot</b>" },
          { fieldId: "f-back", value: "cat" },
        ],
        // What was typed in the tag box and not yet a chip is included.
        tags: ["animals", "pl"],
      },
      expect.anything(),
    );
    expect(field("Front").value).toBe("");
    expect(field("Back").value).toBe("");
    expect((screen.getByLabelText("Deck") as HTMLSelectElement).value).toBe("d-verbs");
    expect((screen.getByLabelText("Type") as HTMLSelectElement).value).toBe("t-basic");
    expect(screen.getByRole("list", { name: "Tags on this note" }).textContent).toContain(
      "animals",
    );
    expect(screen.getByRole("list", { name: "Tags on this note" }).textContent).toContain("pl");
    // Focus goes back to the first field.
    const first = screen.getByRole("textbox", { name: "Front" }).getAttribute("aria-labelledby");
    expect(focusSpies.get(first ?? "")).toHaveBeenCalled();
  });

  test("says one card, not 1 cards", async () => {
    setup();
    await ready();
    type("Front", "x");
    fireEvent.click(addButton());
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Added (1 card)."));
  });

  test("a second tap while adding does not add twice", async () => {
    let finish: (value: unknown) => void = () => {};
    const { addNote } = setup({
      addNote: () => new Promise((resolve) => (finish = resolve)) as never,
    });
    await ready();
    type("Front", "x");
    fireEvent.click(addButton());
    fireEvent.click(addButton());
    expect(addButton().disabled).toBe(true);
    await act(async () => finish({ noteId: "n", cardCount: 1, duplicates: [] }));
    expect(addNote).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(addButton().disabled).toBe(false));
  });

  test("Ctrl+Enter adds from a field", async () => {
    const { addNote } = setup();
    await ready();
    type("Front", "x");
    fireEvent.keyDown(field("Front"), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(addNote).toHaveBeenCalledTimes(1));
  });

  test("Enter in the tag box makes a chip and does not add the note", async () => {
    const { addNote } = setup();
    await ready();
    const tags = screen.getByLabelText("Tags");
    fireEvent.change(tags, { target: { value: "animals" } });
    fireEvent.keyDown(tags, { key: "Enter" });
    expect(screen.getByRole("button", { name: "Remove tag animals" })).toBeTruthy();
    expect(addNote).not.toHaveBeenCalled();
  });

  test("an error from the core is shown as an alert, and what was typed stays", async () => {
    const message = "This note would make no cards. Fill in the front.";
    setup({
      addNote: () => {
        throw { kind: "invalidInput", message };
      },
    });
    await ready();
    type("Back", "cat");
    fireEvent.click(addButton());
    expect((await screen.findByRole("alert")).textContent).toContain(message);
    expect(field("Back").value).toBe("cat");
    expect(addButton().disabled).toBe(false);
  });

  test("a note the core says has a duplicate says so after adding", async () => {
    setup({ addNote: () => ({ noteId: "n", cardCount: 1, duplicates: ["other"] }) });
    await ready();
    type("Front", "kot");
    fireEvent.click(addButton());
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toBe(
        "Added (1 card). Another note with this front already exists.",
      ),
    );
  });
});

describe("the duplicate warning", () => {
  test("shows 400 ms after typing stops, under the first field, and never blocks Add", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { findDuplicates, addNote } = setup({ findDuplicates: () => ["n-old"] });
    await ready();
    type("Front", "kot");
    expect(findDuplicates).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(399);
    });
    expect(findDuplicates).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2);
    });
    expect(findDuplicates).toHaveBeenCalledWith(
      { noteTypeId: "t-basic", value: "kot" },
      expect.anything(),
    );
    expect(await screen.findByText("A note with this front already exists.")).toBeTruthy();
    expect(addButton().disabled).toBe(false);
    fireEvent.click(addButton());
    await waitFor(() => expect(addNote).toHaveBeenCalled());
  });

  test("goes away when the first field is emptied, and asks again only once for fast typing", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { findDuplicates } = setup({ findDuplicates: () => ["n-old"] });
    await ready();
    type("Front", "k");
    type("Front", "ko");
    type("Front", "kot");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(450);
    });
    expect(findDuplicates).toHaveBeenCalledTimes(1);
    await screen.findByText("A note with this front already exists.");
    type("Front", "");
    await waitFor(() => expect(screen.queryByText(/already exists/)).toBeNull());
  });

  test("names the first field by its own name", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setup({ findDuplicates: () => ["n-old"] });
    await ready();
    fireEvent.change(screen.getByLabelText("Type"), { target: { value: "t-cloze" } });
    type("Text", "{{c1::x}}");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(450);
    });
    expect(await screen.findByText("A note with this text already exists.")).toBeTruthy();
  });
});

describe("remembered choices and the draft", () => {
  test("the deck and note type are remembered for the next visit", async () => {
    const first = setup();
    await ready();
    fireEvent.change(screen.getByLabelText("Deck"), { target: { value: "d-polish" } });
    fireEvent.change(screen.getByLabelText("Type"), { target: { value: "t-cloze" } });
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem("fc.add.last") ?? "{}")).toEqual({
        deckId: "d-polish",
        noteTypeId: "t-cloze",
      }),
    );
    first.unmount();
    setup();
    await ready();
    expect((screen.getByLabelText("Deck") as HTMLSelectElement).value).toBe("d-polish");
    expect((screen.getByLabelText("Type") as HTMLSelectElement).value).toBe("t-cloze");
  });

  test("a remembered deck or note type that is gone falls back to Default and Basic", async () => {
    localStorage.setItem("fc.add.last", JSON.stringify({ deckId: "gone", noteTypeId: "gone" }));
    setup();
    await ready();
    await waitFor(() =>
      expect((screen.getByLabelText("Deck") as HTMLSelectElement).value).toBe("d-default"),
    );
    expect((screen.getByLabelText("Type") as HTMLSelectElement).value).toBe("t-basic");
  });

  test("what was typed comes back when the screen is opened again", async () => {
    const first = setup();
    await ready();
    type("Front", "kot");
    fireEvent.change(screen.getByLabelText("Tags"), { target: { value: "animals " } });
    first.unmount(); // leaving the screen saves the draft
    setup();
    await ready();
    expect(field("Front").value).toBe("kot");
    expect(screen.getByRole("button", { name: "Remove tag animals" })).toBeTruthy();
  });

  test("a draft is not read when it is damaged", async () => {
    localStorage.setItem("fc.add.draft", "{not json");
    setup();
    await ready();
    expect(field("Front").value).toBe("");
  });

  test("the draft is cleared by Add and by Clear", async () => {
    setup();
    await ready();
    type("Front", "kot");
    fireEvent.click(addButton());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Added"));
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem("fc.add.draft") ?? "{}").fields).toEqual({}),
    );
    type("Front", "again");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(field("Front").value).toBe("");
    await waitFor(() =>
      expect(JSON.parse(localStorage.getItem("fc.add.draft") ?? "{}")).toMatchObject({
        fields: {},
        tags: [],
      }),
    );
  });

  test("changing the note type keeps values with the same field name and holds the rest", async () => {
    setup();
    await ready();
    type("Front", "kot");
    type("Back", "cat");
    fireEvent.change(screen.getByLabelText("Type"), { target: { value: "t-word" } });
    expect(field("Front").value).toBe("kot");
    expect(field("Notes").value).toBe("");
    type("Notes", "a pet");
    fireEvent.change(screen.getByLabelText("Type"), { target: { value: "t-basic" } });
    expect(field("Front").value).toBe("kot");
    expect(field("Back").value).toBe("cat");
  });
});

describe("the toolbar and the note type", () => {
  test("Cloze is offered for a Cloze note type only", async () => {
    setup();
    await ready();
    expect(screen.queryByRole("button", { name: "Cloze" })).toBeNull();
    fireEvent.change(screen.getByLabelText("Type"), { target: { value: "t-cloze" } });
    expect(screen.getByRole("button", { name: "Cloze" })).toBeTruthy();
  });

  test("the formatting buttons are in the bottom action, with Add", async () => {
    setup();
    await ready();
    const bar = screen.getByRole("toolbar", { name: "Formatting" }).closest(".shell-action");
    expect(bar).toBeTruthy();
    expect(bar?.contains(addButton())).toBe(true);
    for (const name of ["Bold", "Italic", "Bullet list", "Numbered list"]) {
      expect(screen.getByRole("button", { name }).getAttribute("aria-pressed")).toBe("false");
    }
  });
});

describe("tags", () => {
  test("suggests tags in use by prefix, ignoring case, and not ones already on the note", async () => {
    setup();
    await ready();
    const tags = screen.getByLabelText("Tags");
    fireEvent.focus(tags);
    fireEvent.change(tags, { target: { value: "AN" } });
    const names = screen
      .getAllByRole("button")
      .filter((b) => b.closest("ul")?.getAttribute("aria-label") === "Tag suggestions")
      .map((b) => b.textContent);
    expect(names).toEqual(["animals", "Anatomy"]);
    fireEvent.click(screen.getByRole("button", { name: "animals" }));
    expect(screen.getByRole("button", { name: "Remove tag animals" })).toBeTruthy();
  });

  test("a chip is removed with its button and the last one with Backspace", async () => {
    setup();
    await ready();
    const tags = screen.getByLabelText("Tags");
    for (const tag of ["a", "b", "c"]) fireEvent.change(tags, { target: { value: `${tag} ` } });
    fireEvent.click(screen.getByRole("button", { name: "Remove tag b" }));
    fireEvent.keyDown(tags, { key: "Backspace" });
    expect(screen.queryByRole("button", { name: "Remove tag c" })).toBeNull();
    expect(screen.getByRole("button", { name: "Remove tag a" })).toBeTruthy();
  });

  test("a tag is not added twice, whatever its case", async () => {
    const { addNote } = setup();
    await ready();
    const tags = screen.getByLabelText("Tags");
    fireEvent.change(tags, { target: { value: "pl " } });
    fireEvent.change(tags, { target: { value: "PL" } });
    type("Front", "x");
    fireEvent.click(addButton());
    await waitFor(() => expect(addNote).toHaveBeenCalled());
    expect(addNote.mock.calls[0]?.[0].tags).toEqual(["pl"]);
  });

  test("a bad tag shows the core's message", async () => {
    setup({
      addNote: () => {
        throw {
          kind: "invalidInput",
          message: 'The tag "a::" has an empty part. Put a name on both sides of every "::".',
        };
      },
    });
    await ready();
    type("Front", "x");
    fireEvent.change(screen.getByLabelText("Tags"), { target: { value: "a::" } });
    fireEvent.click(addButton());
    expect((await screen.findByRole("alert")).textContent).toContain("has an empty part");
  });
});

describe("when the collection cannot be read", () => {
  test("says so and offers to try again", async () => {
    let fail = true;
    setup({
      getDeckList: () => {
        if (fail) throw { kind: "internal", message: "Something went wrong inside the app." };
        return { decks, totalCards: 0 };
      },
    });
    expect((await screen.findByRole("alert")).textContent).toContain("Something went wrong");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await ready();
  });
});
