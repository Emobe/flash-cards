import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type NoteTypeDetails,
  type NoteTypeDraft,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../../core";
import { RouterProvider } from "../../router";
import { AppShell } from "../../shell/AppShell";
import { NoteTypeScreen } from "./NoteTypeScreen";

afterEach(() => {
  cleanup();
  window.location.hash = "";
});

const draft = (): NoteTypeDraft => ({
  name: "Polish words",
  css: ".card {}",
  sortField: 0,
  fields: [
    { id: "f1", name: "Front" },
    { id: "f2", name: "Back" },
  ],
  templates: [{ id: "t1", name: "Card 1", front: "{{Front}}", back: "{{FrontSide}} {{Back}}" }],
});

const details = (over: Partial<NoteTypeDetails> = {}): NoteTypeDetails => ({
  id: "nt",
  kind: "standard",
  noteCount: 120,
  draft: draft(),
  removedFields: [],
  removedTemplates: [],
  ...over,
});

const token = { id: "u1", message: "Saved Polish words." };

function wide(on: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: on && query === "(min-width: 600px)",
    addEventListener() {},
    removeEventListener() {},
  })) as unknown as typeof window.matchMedia;
}

beforeEach(() => wide(false));

function setup(
  handlers: Record<string, (input: never) => unknown> = {},
  loaded: NoteTypeDetails = details(),
) {
  const calls = {
    getNoteTypeDetails: vi.fn(() => loaded),
    checkNoteType: vi.fn((_: unknown) => ({ cardsAdded: 0, cardsRemoved: 0, notesAffected: 0 })),
    saveNoteType: vi.fn((_: unknown) => ({
      message: "Saved Polish words.",
      cardsAdded: 0,
      cardsRemoved: 0,
      undo: token,
    })),
    renameFieldInTemplates: vi.fn((input: { templates: unknown[]; from: string; to: string }) => ({
      templates: input.templates.map((t) => {
        const template = t as { front: string; back: string };
        return {
          ...template,
          front: template.front.replaceAll(`{{${input.from}}}`, `{{${input.to}}}`),
          back: template.back.replaceAll(`{{${input.from}}}`, `{{${input.to}}}`),
        };
      }),
    })),
    undoChange: vi.fn((_: unknown) => ({
      message: "Put back the earlier version of Polish words.",
    })),
    deleteNoteType: vi.fn((_: unknown) => ({
      undo: { id: "u2", message: "Deleted Polish words." },
    })),
    ...handlers,
  };
  const client = new CoreClient(createFakeTransport(calls as never));
  render(
    <CoreProvider client={client}>
      <RouterProvider>
        <AppShell>
          <NoteTypeScreen noteTypeId="nt" />
        </AppShell>
      </RouterProvider>
    </CoreProvider>,
  );
  return calls;
}

const ready = () => screen.findByRole("textbox", { name: "Name" });
const save = () => screen.getByRole("button", { name: "Save" });

/** The rows of a group, by name. */
const rowNames = (legend: string) => {
  const group = screen.getByText(legend).closest("fieldset") as HTMLElement;
  return within(group)
    .getAllByRole("listitem")
    .map((li) => li.querySelector(".part-name")?.textContent);
};

describe("a note type on a phone", () => {
  test("shows its name, fields and cards, and Save waits for a change", async () => {
    setup();
    expect(((await ready()) as HTMLInputElement).value).toBe("Polish words");
    expect(rowNames("Fields")).toEqual(["Front", "Back"]);
    expect(rowNames("Cards")).toEqual(["Card 1"]);
    expect(screen.getByText("Sorts notes")).toBeDefined();
    expect((save() as HTMLButtonElement).disabled).toBe(true);
  });

  test("a row's options are in a sheet, and Move down reorders the draft", async () => {
    const calls = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Options for Front" }));
    const sheet = await screen.findByRole("dialog", { name: "Front" });
    expect(within(sheet).getByRole("button", { name: "Move up" })).toHaveProperty("disabled", true);
    fireEvent.click(within(sheet).getByRole("button", { name: "Move down" }));
    await waitFor(() => expect(rowNames("Fields")).toEqual(["Back", "Front"]));
    fireEvent.click(save());
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalled());
    const sent = calls.saveNoteType.mock.calls[0]?.[0] as {
      noteTypeId: string;
      draft: NoteTypeDraft;
    };
    expect(sent.noteTypeId).toBe("nt");
    expect(sent.draft.fields.map((f) => f.name)).toEqual(["Back", "Front"]);
    // The sort field moved with its field.
    expect(sent.draft.sortField).toBe(1);
  });

  test("after Save the notice says what happened and Undo reads the note type again", async () => {
    const calls = setup();
    fireEvent.change(await ready(), { target: { value: "Polish" } });
    fireEvent.click(save());
    const status = await screen.findByText("Saved Polish words.");
    expect(status).toBeDefined();
    expect(calls.checkNoteType).toHaveBeenCalledTimes(1);
    expect(calls.getNoteTypeDetails).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await screen.findByText("Put back the earlier version of Polish words.");
    expect(calls.undoChange.mock.calls[0]?.[0]).toEqual({ undoId: "u1" });
    await waitFor(() => expect(calls.getNoteTypeDetails).toHaveBeenCalledTimes(3));
  });
});

describe("a note type on a wide screen", () => {
  test("rows have their buttons, and Remove stops at the last field", async () => {
    wide(true);
    setup();
    await ready();
    expect(screen.queryByRole("button", { name: "Options for Front" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Remove Back" }));
    expect(rowNames("Fields")).toEqual(["Front"]);
    expect(screen.getByRole("button", { name: "Remove Front" })).toHaveProperty("disabled", true);
    expect(rowNames("Removed fields")).toEqual(["Back"]);
    fireEvent.click(screen.getByRole("button", { name: "Restore Back" }));
    expect(rowNames("Fields")).toEqual(["Front", "Back"]);
  });

  test("renaming a field rewrites the cards in the draft", async () => {
    wide(true);
    const calls = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Rename Front" }));
    const window_ = await screen.findByRole("dialog", { name: "Rename Front" });
    fireEvent.change(within(window_).getByLabelText("Name"), { target: { value: "Word" } });
    fireEvent.click(within(window_).getByRole("button", { name: "Rename" }));
    await waitFor(() => expect(rowNames("Fields")).toEqual(["Word", "Back"]));
    fireEvent.click(save());
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalled());
    const sent = calls.saveNoteType.mock.calls[0]?.[0] as { draft: NoteTypeDraft };
    expect(sent.draft.templates[0]?.front).toBe("{{Word}}");
    expect(sent.draft.fields[0]).toEqual({ id: "f1", name: "Word" });
  });

  test("a repeated name is refused next to the box", async () => {
    wide(true);
    setup();
    await ready();
    fireEvent.change(screen.getByRole("textbox", { name: "Field name" }), {
      target: { value: "back" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add field" }));
    expect((await screen.findByRole("alert")).textContent).toContain('already one called "back"');
    expect(rowNames("Fields")).toEqual(["Front", "Back"]);
  });

  test("Add card copies the first card with the next name", async () => {
    wide(true);
    const calls = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Add card" }));
    expect(rowNames("Cards")).toEqual(["Card 1", "Card 2"]);
    fireEvent.click(save());
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalled());
    const sent = calls.saveNoteType.mock.calls[0]?.[0] as { draft: NoteTypeDraft };
    expect(sent.draft.templates[1]).toEqual({
      id: null,
      name: "Card 2",
      front: "{{Front}}",
      back: "{{FrontSide}} {{Back}}",
    });
  });

  test("a cloze type has no Add card", async () => {
    wide(true);
    setup({}, details({ kind: "cloze" }));
    await ready();
    expect(screen.queryByRole("button", { name: "Add card" })).toBeNull();
    expect(screen.getByRole("button", { name: "Remove Card 1" })).toHaveProperty("disabled", true);
  });
});

describe("saving", () => {
  test("a change that empties cards asks first, and Cancel saves nothing", async () => {
    const calls = setup({
      checkNoteType: () => ({ cardsAdded: 0, cardsRemoved: 12, notesAffected: 12 }),
    });
    fireEvent.change(await ready(), { target: { value: "Polish" } });
    fireEvent.click(save());
    const ask = await screen.findByRole("dialog", { name: "Send cards to the trash?" });
    expect(ask.textContent).toContain("12 cards of 12 notes");
    fireEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.saveNoteType).not.toHaveBeenCalled();

    fireEvent.click(save());
    const again = await screen.findByRole("dialog", { name: "Send cards to the trash?" });
    fireEvent.click(within(again).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalledTimes(1));
  });

  test("a refusal is shown and nothing is saved", async () => {
    const calls = setup({
      checkNoteType: () => {
        throw { kind: "invalidInput", message: "The front of the card template has a mistake." };
      },
    });
    fireEvent.change(await ready(), { target: { value: "Polish" } });
    fireEvent.click(save());
    expect((await screen.findByRole("alert")).textContent).toContain("has a mistake");
    expect(calls.saveNoteType).not.toHaveBeenCalled();
  });

  test("an empty name is refused before asking the core", async () => {
    const calls = setup();
    fireEvent.change(await ready(), { target: { value: "  " } });
    fireEvent.click(save());
    expect((await screen.findByRole("alert")).textContent).toContain("The name is empty");
    expect(calls.checkNoteType).not.toHaveBeenCalled();
  });

  test("Ctrl+S saves", async () => {
    const calls = setup();
    fireEvent.change(await ready(), { target: { value: "Polish" } });
    fireEvent.keyDown(document, { key: "s", ctrlKey: true });
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalled());
  });
});

describe("leaving", () => {
  test("Back with a change asks, Keep editing stays, Discard goes to the list", async () => {
    setup();
    fireEvent.change(await ready(), { target: { value: "Polish" } });
    fireEvent.click(screen.getByRole("button", { name: "Note types" }));
    const ask = await screen.findByRole("dialog", { name: "Discard your changes?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Keep editing" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect((screen.getByRole("textbox", { name: "Name" }) as HTMLInputElement).value).toBe(
      "Polish",
    );
    fireEvent.click(screen.getByRole("button", { name: "Note types" }));
    const again = await screen.findByRole("dialog", { name: "Discard your changes?" });
    fireEvent.click(within(again).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(window.location.hash).toBe("#/notetypes"));
  });

  test("Back with no change leaves at once", async () => {
    setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Note types" }));
    await waitFor(() => expect(window.location.hash).toBe("#/notetypes"));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("deleting", () => {
  test("asks with the counts, then offers Undo", async () => {
    const calls = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Delete note type" }));
    const ask = await screen.findByRole("dialog", {
      name: "Delete Polish words and its 120 notes?",
    });
    fireEvent.click(within(ask).getByRole("button", { name: "Delete" }));
    await screen.findByText("Deleted Polish words.");
    expect(calls.deleteNoteType.mock.calls[0]?.[0]).toEqual({ noteTypeId: "nt" });
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(calls.getNoteTypeDetails).toHaveBeenCalledTimes(2));
  });
});
