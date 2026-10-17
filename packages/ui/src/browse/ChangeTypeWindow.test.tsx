import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, CoreError, createFakeTransport } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { ChangeTypeWindow } from "./ChangeTypeWindow";
import { typeBasic, typeCloze, typeReversed } from "./testTypes";

beforeEach(() => {
  window.history.replaceState(null, "", "/#/browse");
});
afterEach(cleanup);

function setup({
  noteTypeId = typeBasic.id as string | null,
  change = () => ({
    notes: 12,
    carried: 12,
    undo: { id: "u-type", message: "Changed 12 notes to Basic and reversed." },
  }),
}: {
  noteTypeId?: string | null;
  change?: () => unknown;
} = {}) {
  const changeNoteType = vi.fn((_input: unknown) => change() as never);
  const getNoteTypes = vi.fn(() => ({ noteTypes: [typeBasic, typeReversed, typeCloze] }));
  const onDone = vi.fn();
  const onClose = vi.fn();
  const client = new CoreClient(createFakeTransport({ getNoteTypes, changeNoteType }));
  render(
    <CoreProvider client={client}>
      <ChangeTypeWindow
        target={{ notes: ["n1", "n2"] }}
        notes={12}
        noteTypeId={noteTypeId}
        onDone={onDone}
        onClose={onClose}
      />
    </CoreProvider>,
  );
  return { changeNoteType, getNoteTypes, onDone, onClose };
}

const select = async (name: string) =>
  (await screen.findByRole("combobox", { name })) as HTMLSelectElement;
const change = () => fireEvent.click(screen.getByRole("button", { name: "Change" }));

describe("the change note type window", () => {
  test("starts on the first other type with the default maps and says what it will do", async () => {
    setup();
    const to = await select("From Basic to");
    expect(to.value).toBe("nt-2");
    expect((await select("Back")).value).toBe("f1-back");
    expect((await select("Card 1")).value).toBe("t1-card-1");
    expect((await select("Card 2")).value).toBe("");
    expect(screen.getByRole("status").textContent).toBe(
      "12 notes. Card 1 keeps its schedule. Card 2 starts as new.",
    );
  });

  test("Change sends every field and card with the old one that fills it", async () => {
    const { changeNoteType, onDone } = setup();
    await select("Card 2");
    change();
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(changeNoteType.mock.calls[0]?.[0]).toEqual({
      target: { notes: ["n1", "n2"] },
      noteTypeId: "nt-2",
      fields: [
        { fieldId: "f2-front", from: "f1-front" },
        { fieldId: "f2-back", from: "f1-back" },
      ],
      templates: [
        { templateId: "t2-card-1", from: "t1-card-1" },
        { templateId: "t2-card-2", from: null },
      ],
    });
    expect(onDone.mock.calls[0]?.[0]).toMatchObject({ notes: 12, carried: 12 });
  });

  test("a field set to Nothing is sent as null and the summary says so", async () => {
    const { changeNoteType } = setup();
    fireEvent.change(await select("Back"), { target: { value: "" } });
    expect(screen.getByRole("status").textContent).toContain(
      "Back is not used. Back is left empty.",
    );
    change();
    await waitFor(() => expect(changeNoteType).toHaveBeenCalled());
    expect(changeNoteType.mock.calls[0]?.[0]).toMatchObject({
      fields: [
        { fieldId: "f2-front", from: "f1-front" },
        { fieldId: "f2-back", from: null },
      ],
    });
  });

  test("an old card already chosen for one card cannot be chosen for another", async () => {
    setup();
    const second = await select("Card 2");
    const taken = within(second).getByRole("option", { name: "Card 1" }) as HTMLOptionElement;
    expect(taken.disabled).toBe(true);
    const first = await select("Card 1");
    expect(within(first).getByRole("option", { name: "Card 1" })).toHaveProperty("disabled", false);
    fireEvent.change(first, { target: { value: "" } });
    expect(taken.disabled).toBe(false);
    fireEvent.change(second, { target: { value: "t1-card-1" } });
    expect(screen.getByRole("status").textContent).toContain(
      "Card 2 takes the schedule of Card 1.",
    );
    expect(screen.getByRole("status").textContent).toContain("Card 1 starts as new.");
  });

  test("another type brings its own maps, and between kinds no card is chosen", async () => {
    const { changeNoteType } = setup();
    const to = await select("From Basic to");
    fireEvent.change(to, { target: { value: "nt-3" } });
    expect(screen.queryByRole("combobox", { name: "Card 1" })).toBeNull();
    expect((await select("Text")).value).toBe("f1-front");
    expect(screen.getByRole("status").textContent).toContain("Every card starts as new");
    change();
    await waitFor(() => expect(changeNoteType).toHaveBeenCalled());
    expect(changeNoteType.mock.calls[0]?.[0]).toMatchObject({ noteTypeId: "nt-3", templates: [] });
  });

  test("a refusal is shown and the window stays open", async () => {
    const { onDone } = setup({
      change: () => {
        throw new CoreError(
          "invalidInput",
          "1 note would be left with no cards. Choose a field for what the front of the new cards shows.",
        );
      },
    });
    await select("Card 2");
    change();
    expect((await screen.findByRole("alert")).textContent).toContain("left with no cards");
    expect(onDone).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Change note type" })).not.toBeNull();
  });

  test("notes of several types are told to select one type", async () => {
    const { getNoteTypes, onClose } = setup({ noteTypeId: null });
    expect(screen.getByText("Select notes of one note type to change it.")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Change" })).toBeNull();
    expect(getNoteTypes).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  test("Back and Escape close it", async () => {
    const { onClose } = setup();
    await select("Card 2");
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent(
      screen.getByRole("dialog", { name: "Change note type" }),
      new Event("cancel", { cancelable: true }),
    );
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
