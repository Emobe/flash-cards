import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, createFakeTransport } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { type EditorResult, NoteEditor } from "./NoteEditor";
import { editorHandlers } from "./testEditor";

vi.mock("./FieldEditor", () => import("./testEditor"));

beforeEach(() => {
  window.history.replaceState(null, "", "/#/browse");
});
afterEach(cleanup);

function setup(note: Parameters<typeof editorHandlers>[0] = {}) {
  const handlers = editorHandlers(note);
  const onClose = vi.fn();
  const onDone = vi.fn((_result: EditorResult) => {});
  const client = new CoreClient(createFakeTransport(handlers));
  render(
    <CoreProvider client={client}>
      <NoteEditor noteId="n1" onClose={onClose} onDone={onDone} />
    </CoreProvider>,
  );
  return { ...handlers, onClose, onDone };
}

async function field(name: string): Promise<HTMLTextAreaElement> {
  return (await screen.findByLabelText(name, { selector: "textarea" })) as HTMLTextAreaElement;
}

function save() {
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
}

/** Escape on the editor's window, as the browser sends it. */
function pressEscape() {
  const dialog = screen.getByRole("dialog", { name: "Edit note" });
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
}

describe("opening", () => {
  test("loads the note with its type, fields, tags and deck", async () => {
    const { getNote } = setup();
    expect(await field("Front")).toHaveProperty("value", "kot");
    expect((await field("Back")).value).toBe("cat");
    expect(getNote).toHaveBeenCalledWith({ noteId: "n1" }, expect.anything());
    expect(screen.getByText("Basic")).not.toBeNull();
    expect(screen.getByText("animals")).not.toBeNull();
    expect((screen.getByLabelText("Deck") as HTMLSelectElement).value).toBe("d-default");
  });

  test("cards in several decks read Several decks until a deck is chosen", async () => {
    setup({ deckIds: ["d-default", "d-polish"] });
    const deck = (await screen.findByLabelText("Deck")) as HTMLSelectElement;
    expect(deck.value).toBe("");
    expect(deck.selectedOptions[0]?.textContent).toBe("Several decks");
  });

  test("a field the editor cannot show opens as its HTML in a text box, as text", async () => {
    const html = '<table><tr><td>kot</td></tr></table><img src="x" onerror="alert(1)">';
    setup({ back: html });
    const box = await field("Back");
    expect(box.tagName).toBe("TEXTAREA");
    expect(box.value).toBe(html);
    expect(screen.getByText("This field has formatting the editor cannot show.")).not.toBeNull();
    // Shown as text: none of it became an element.
    expect(document.querySelector("table, img")).toBeNull();
  });

  test("the editor never puts field HTML in the page with innerHTML", () => {
    for (const file of ["NoteEditor.tsx", "useFieldEditing.ts", "../components/Dialog.tsx"]) {
      const source = readFileSync(join(import.meta.dirname, file), "utf8");
      expect(source, file).not.toMatch(/innerHTML|dangerouslySetInnerHTML/);
    }
  });
});

describe("saving", () => {
  test("sends one updateNote with only the changed field and every tag", async () => {
    const { updateNote, onDone } = setup();
    fireEvent.change(await field("Back"), { target: { value: "a cat" } });
    save();
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(updateNote).toHaveBeenCalledTimes(1);
    expect(updateNote.mock.calls[0]?.[0]).toEqual({
      noteId: "n1",
      fields: [{ fieldId: "f-back", value: "a cat" }],
      tags: ["animals"],
    });
    expect(onDone).toHaveBeenCalledWith({
      kind: "saved",
      undo: { id: "u1", message: "Saved." },
      cardsRemoved: [],
    });
  });

  test("saving another field leaves the HTML box's field out", async () => {
    const { updateNote } = setup({ back: "<table><tr><td>cat</td></tr></table>" });
    fireEvent.change(await field("Front"), { target: { value: "pies" } });
    save();
    await waitFor(() => expect(updateNote).toHaveBeenCalled());
    expect(updateNote.mock.calls[0]?.[0].fields).toEqual([{ fieldId: "f-front", value: "pies" }]);
  });

  test("a new deck goes with the save, so one Undo covers both", async () => {
    const { updateNote } = setup();
    fireEvent.change(await screen.findByLabelText("Deck"), { target: { value: "d-polish" } });
    save();
    await waitFor(() => expect(updateNote).toHaveBeenCalled());
    expect(updateNote.mock.calls[0]?.[0]).toMatchObject({ fields: [], deckId: "d-polish" });
  });

  test("a tag typed and not yet a chip is saved", async () => {
    const { updateNote } = setup();
    fireEvent.change(await screen.findByLabelText("Tags"), { target: { value: "pl" } });
    save();
    await waitFor(() => expect(updateNote).toHaveBeenCalled());
    expect(updateNote.mock.calls[0]?.[0].tags).toEqual(["animals", "pl"]);
  });

  test("Save with nothing changed closes without a write", async () => {
    const { updateNote, onClose } = setup();
    await field("Front");
    save();
    expect(onClose).toHaveBeenCalled();
    expect(updateNote).not.toHaveBeenCalled();
  });

  test("Ctrl+S and Ctrl+Enter save", async () => {
    const { updateNote } = setup();
    const front = await field("Front");
    fireEvent.change(front, { target: { value: "pies" } });
    fireEvent.keyDown(front, { key: "s", ctrlKey: true });
    await waitFor(() => expect(updateNote).toHaveBeenCalledTimes(1));
    fireEvent.keyDown(front, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(updateNote).toHaveBeenCalledTimes(2));
  });

  test("a refused save stays open and says why", async () => {
    const { updateNote, onDone } = setup();
    updateNote.mockImplementation(() => {
      throw { kind: "invalidInput", message: "This note would make no cards." };
    });
    fireEvent.change(await field("Front"), { target: { value: "" } });
    save();
    expect((await screen.findByRole("alert")).textContent).toContain("would make no cards");
    expect(onDone).not.toHaveBeenCalled();
  });
});

describe("closing", () => {
  test("Escape with no changes just closes", async () => {
    const { onClose } = setup();
    await field("Front");
    pressEscape();
    expect(onClose).toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Discard your changes?" })).toBeNull();
  });

  test("Escape with changes asks first: Keep editing stays, Discard closes", async () => {
    const { onClose, updateNote } = setup();
    fireEvent.change(await field("Front"), { target: { value: "pies" } });
    pressEscape();
    const ask = screen.getByRole("dialog", { name: "Discard your changes?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Keep editing" }));
    expect(screen.queryByRole("dialog", { name: "Discard your changes?" })).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect((await field("Front")).value).toBe("pies");

    pressEscape();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(onClose).toHaveBeenCalled();
    expect(updateNote).not.toHaveBeenCalled();
  });

  test("Escape on the question answers it, even where the browser bubbles cancel", async () => {
    const { onClose } = setup();
    fireEvent.change(await field("Front"), { target: { value: "pies" } });
    pressEscape();
    const ask = screen.getByRole("dialog", { name: "Discard your changes?" });
    fireEvent(ask, new Event("cancel", { cancelable: true, bubbles: true }));
    expect(screen.queryByRole("dialog", { name: "Discard your changes?" })).toBeNull();
    expect(screen.getByRole("dialog", { name: "Edit note" })).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  test("Back with changes asks, and the editor keeps a history entry for the next Back", async () => {
    const { onClose } = setup();
    fireEvent.change(await field("Front"), { target: { value: "pies" } });
    expect(window.history.state?.fcDialog).toBe(1);
    await act(async () => {
      window.history.replaceState(null, "");
      window.dispatchEvent(new PopStateEvent("popstate"));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.getByRole("dialog", { name: "Discard your changes?" })).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    // The editor's entry again, and one for the question above it.
    expect(window.history.state?.fcDialog).toBe(2);
    await act(async () => {
      window.history.back();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(screen.queryByRole("dialog", { name: "Discard your changes?" })).toBeNull();
    expect(screen.getByRole("dialog", { name: "Edit note" })).not.toBeNull();
    expect(window.history.state?.fcDialog).toBe(1);
  });

  test("the Back control closes when nothing changed", async () => {
    const { onClose } = setup();
    await field("Front");
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(onClose).toHaveBeenCalled();
  });
});

describe("the rest", () => {
  test("the duplicate warning leaves out the note itself", async () => {
    const { findDuplicates } = setup();
    findDuplicates.mockImplementation(() => ({ noteIds: ["n1"] }));
    await field("Front");
    await waitFor(() => expect(findDuplicates).toHaveBeenCalled(), { timeout: 2000 });
    expect(screen.queryByText(/already exists/)).toBeNull();

    findDuplicates.mockImplementation(() => ({ noteIds: ["n1", "n2"] }));
    fireEvent.change(await field("Front"), { target: { value: "pies" } });
    expect(
      await screen.findByText("Another note with this front already exists.", undefined, {
        timeout: 2000,
      }),
    ).not.toBeNull();
  });

  test("Delete asks with the card count, then deletes the note", async () => {
    const { deleteNotes, onDone } = setup({ cardCount: 2 });
    await field("Front");
    fireEvent.click(screen.getByRole("button", { name: "Delete note" }));
    const ask = screen.getByRole("dialog", { name: "Delete this note and its 2 cards?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(deleteNotes.mock.calls[0]?.[0]).toEqual({ target: { notes: ["n1"] } });
    expect(onDone).toHaveBeenCalledWith({
      kind: "deleted",
      undo: { id: "u2", message: "Deleted 1 note and its 1 card." },
    });
  });
});
