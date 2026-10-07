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
vi.mock("../editor/media", async (original) => ({
  ...(await original<typeof import("../editor/media")>()),
  // happy-dom has no canvas; the real preparation is tested in media.test.ts and in a browser.
  prepareImage: vi.fn(async (file: File) => {
    if (file.name === "bad.heic") {
      const { MediaRefused } = await import("../editor/media");
      throw new MediaRefused("This picture's format can't be shown on cards. Try a JPEG or PNG.");
    }
    return { name: file.name, bytes: new Uint8Array([1, 2, 3]), type: "image/png" };
  }),
}));
vi.mock("../editor/FieldEditor", async () => {
  const { useEffect, useRef } = await import("react");
  const { EditorState } = await import("prosemirror-state");
  const { loadField, saveField } = await import("../editor/html");
  return {
    FieldEditor: ({
      value,
      onChange,
      labelId,
      loadMedia,
      onReady,
    }: {
      value: string;
      onChange: (html: string) => void;
      labelId: string;
      loadMedia: (name: string) => Promise<Blob>;
      onReady?: (view: unknown) => void;
    }) => {
      // A real editor state under a text area, so inserting a picture makes the real HTML.
      const state = useRef(EditorState.create({ doc: loadField(value) }));
      if (saveField(state.current.doc) !== value) {
        state.current = EditorState.create({ doc: loadField(value) });
      }
      const change = useRef(onChange);
      change.current = onChange;
      // Once per mount, like the real editor (its `onReady` is read through a ref).
      useEffect(() => {
        const focus = vi.fn();
        focusSpies.set(labelId, focus);
        const dom = document.createElement("div");
        document.body.append(dom);
        onReady?.({
          focus,
          dom,
          get state() {
            return state.current;
          },
          dispatch: (tr: import("prosemirror-state").Transaction) => {
            state.current = state.current.apply(tr);
            change.current(saveField(state.current.doc));
          },
        });
        return () => {
          dom.remove();
          onReady?.(null);
        };
      }, [labelId]);
      // A restored draft with a picture asks for it, as the node view does.
      useEffect(() => {
        for (const match of value.matchAll(/<img src="([^"]+)"/g))
          void loadMedia(match[1] as string);
      }, [value, loadMedia]);
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
    addMedia?: (input: { name: string }) => unknown;
  } = {},
) {
  const handlers = {
    getDeckList: vi.fn(overrides.getDeckList ?? (() => ({ decks, totalCards: 0 }))),
    getNoteTypes: vi.fn(() => ({ noteTypes })),
    getTags: vi.fn(() => ({ tags: ["animals", "Anatomy", "pl", "pl::verbs"] })),
    findDuplicates: vi.fn((input: { noteTypeId: string; value: string }) => ({
      noteIds: overrides.findDuplicates?.(input) ?? [],
    })),
    addMedia: vi.fn((input: { name: string }, ctx: { bytes?: Uint8Array }) => {
      // The web client transfers the bytes to its worker, which leaves the sender's buffer empty.
      if (ctx.bytes) structuredClone(ctx.bytes.buffer, { transfer: [ctx.bytes.buffer] });
      return (
        overrides.addMedia?.(input) ?? {
          name: input.name.replace(/(\.[^.]+)?$/, "-0123456789abcdef$1"),
          new: true,
        }
      );
    }),
    getMedia: vi.fn((_input: { name: string }) => ({ contentType: "image/png" })),
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

  test("the added line goes away when the next note is started", async () => {
    setup();
    await ready();
    type("Front", "x");
    fireEvent.click(addButton());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Added"));
    type("Front", "y");
    expect(screen.getByRole("status").textContent).toBe("");
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

describe("pictures and sounds", () => {
  const imageInput = () =>
    document.querySelector(
      'input[type="file"][accept="image/*"]:not([capture])',
    ) as HTMLInputElement;
  const cameraInput = () =>
    document.querySelector('input[type="file"][capture]') as HTMLInputElement | null;
  const soundInput = () =>
    document.querySelector('input[type="file"][accept="audio/*"]') as HTMLInputElement;
  const pick = (input: HTMLInputElement, ...files: File[]) =>
    fireEvent.change(input, { target: { files } });
  const png = (name: string) => new File([new Uint8Array([1, 2, 3])], name, { type: "image/png" });

  test("the Image button opens the picker", async () => {
    setup();
    await ready();
    const click = vi.spyOn(imageInput(), "click");
    fireEvent.click(screen.getByRole("button", { name: "Image" }));
    expect(click).toHaveBeenCalled();
  });

  test("a picked picture is stored when it is picked, and the field gets the name the core gave it", async () => {
    const { addMedia, addNote } = setup();
    await ready();
    pick(imageInput(), png("cat.png"));
    await waitFor(() => expect(field("Front").value).toBe('<img src="cat-0123456789abcdef.png">'));
    expect(addMedia).toHaveBeenCalledTimes(1);
    expect(addMedia.mock.calls[0]?.[0]).toEqual({ name: "cat.png" });
    expect(addNote).not.toHaveBeenCalled();
    fireEvent.click(addButton());
    await waitFor(() => expect(addNote).toHaveBeenCalled());
    expect(addNote.mock.calls[0]?.[0].fields[0]).toEqual({
      fieldId: "f-front",
      value: '<img src="cat-0123456789abcdef.png">',
    });
  });

  test("several pictures are stored one after another and all go in", async () => {
    const { addMedia } = setup();
    await ready();
    pick(imageInput(), png("a.png"), png("b.png"));
    await waitFor(() =>
      expect(field("Front").value).toBe(
        '<img src="a-0123456789abcdef.png"><img src="b-0123456789abcdef.png">',
      ),
    );
    expect(addMedia).toHaveBeenCalledTimes(2);
  });

  test("a picture is added to what is already in the field, not over it", async () => {
    setup();
    await ready();
    type("Front", "cat ");
    pick(imageInput(), png("a.png"));
    await waitFor(() => expect(field("Front").value).toContain("<img"));
    expect(field("Front").value).toContain("cat ");
  });

  test("a sound is added as [sound:name]", async () => {
    const { addMedia } = setup();
    await ready();
    pick(soundInput(), new File([new Uint8Array([1])], "meow.mp3", { type: "audio/mpeg" }));
    await waitFor(() => expect(field("Front").value).toBe("[sound:meow-0123456789abcdef.mp3]"));
    expect(addMedia).toHaveBeenCalledTimes(1);
  });

  test("a file that cannot be used is refused with the reason, and nothing is stored", async () => {
    const { addMedia } = setup();
    await ready();
    pick(imageInput(), png("bad.heic"));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "This picture's format can't be shown on cards",
    );
    expect(addMedia).not.toHaveBeenCalled();
    expect(field("Front").value).toBe("");
  });

  test("one bad file among several does not stop the others", async () => {
    setup();
    await ready();
    pick(imageInput(), png("bad.heic"), png("ok.png"));
    await waitFor(() => expect(field("Front").value).toBe('<img src="ok-0123456789abcdef.png">'));
    expect((await screen.findByRole("alert")).textContent).toContain("can't be shown");
  });

  test("a failure in the core is shown and the field is left alone", async () => {
    setup({
      addMedia: () => {
        throw { kind: "internal", message: "Something went wrong inside the app." };
      },
    });
    await ready();
    type("Front", "kept");
    pick(imageInput(), png("a.png"));
    expect((await screen.findByRole("alert")).textContent).toContain("Something went wrong");
    expect(field("Front").value).toBe("kept");
  });

  test("the buttons and Add wait while files are being stored", async () => {
    let finish: (value: unknown) => void = () => {};
    const { addNote } = setup({
      addMedia: () => new Promise((resolve) => (finish = resolve)) as never,
    });
    await ready();
    type("Front", "x");
    pick(imageInput(), png("a.png"));
    await waitFor(() => expect(addButton().disabled).toBe(true));
    expect((screen.getByRole("button", { name: "Image" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(screen.getByRole("status").textContent).toBe("Adding the file...");
    fireEvent.keyDown(field("Front"), { key: "Enter", ctrlKey: true });
    expect(addNote).not.toHaveBeenCalled();
    await act(async () => finish({ name: "a-0123456789abcdef.png", new: true }));
    await waitFor(() => expect(addButton().disabled).toBe(false));
    expect(screen.getByRole("status").textContent).toBe("");
  });

  test("a picture in a restored draft is fetched for showing, once, and one just added is not", async () => {
    localStorage.setItem(
      "fc.add.draft",
      JSON.stringify({
        deckId: "d-default",
        noteTypeId: "t-basic",
        fields: { Front: '<img src="old-0123456789abcdef.png">' },
        tags: [],
      }),
    );
    const { getMedia } = setup();
    await ready();
    await waitFor(() => expect(getMedia).toHaveBeenCalledTimes(1));
    expect(getMedia.mock.calls[0]?.[0]).toEqual({ name: "old-0123456789abcdef.png" });
    pick(imageInput(), png("new.png"));
    await waitFor(() => expect(field("Front").value).toContain("new-0123456789abcdef.png"));
    expect(getMedia).toHaveBeenCalledTimes(1);
  });

  test("Take photo is offered on a touch screen only, and asks for the back camera", async () => {
    setup();
    await ready();
    expect(screen.queryByRole("button", { name: "Take photo" })).toBeNull();
    expect(cameraInput()?.getAttribute("capture")).toBe("environment");
    cleanup();
    const real = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches: query === "(pointer: coarse)",
      addEventListener() {},
      removeEventListener() {},
    })) as unknown as typeof window.matchMedia;
    try {
      setup();
      await ready();
      const click = vi.spyOn(cameraInput() as HTMLInputElement, "click");
      fireEvent.click(screen.getByRole("button", { name: "Take photo" }));
      expect(click).toHaveBeenCalled();
    } finally {
      window.matchMedia = real;
    }
  });

  test("a photo from the camera goes through the same steps", async () => {
    const { addMedia } = setup();
    await ready();
    pick(cameraInput() as HTMLInputElement, png("JPEG_2026_1.png"));
    await waitFor(() => expect(addMedia).toHaveBeenCalledTimes(1));
    expect(field("Front").value).toContain("<img");
  });
});
