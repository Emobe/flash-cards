import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, createFakeTransport, type NoteTypeSummary } from "core-client";
import { afterEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../../core";
import { RouterProvider } from "../../router";
import { AppShell } from "../../shell/AppShell";
import { NoteTypeList } from "./NoteTypeList";

afterEach(() => {
  cleanup();
  window.location.hash = "";
});

const type = (id: string, name: string, kind: "standard" | "cloze", noteCount: number) =>
  ({ id, name, kind, fields: [], templates: [], noteCount }) satisfies NoteTypeSummary;

function setup(noteTypes: NoteTypeSummary[]) {
  const createNoteType = vi.fn((_: unknown) => ({
    noteTypeId: "new-1",
    undo: { id: "u", message: "Added." },
  }));
  const client = new CoreClient(
    createFakeTransport({ getNoteTypes: () => ({ noteTypes }), createNoteType } as never),
  );
  render(
    <CoreProvider client={client}>
      <RouterProvider>
        <AppShell>
          <NoteTypeList />
        </AppShell>
      </RouterProvider>
    </CoreProvider>,
  );
  return createNoteType;
}

describe("the note type list", () => {
  test("lists each type with its kind and notes, linking to its screen", async () => {
    setup([type("a", "Basic", "standard", 1), type("b", "Gaps", "cloze", 40)]);
    const basic = await screen.findByRole("link", { name: /Basic/ });
    expect(basic.textContent).toContain("Standard · 1 note");
    expect(basic.getAttribute("href")).toBe("#/notetypes/a");
    expect(screen.getByRole("link", { name: /Gaps/ }).textContent).toContain("Cloze · 40 notes");
  });

  test("New note type starts from a copy and opens the new one", async () => {
    const create = setup([type("a", "Basic", "standard", 1), type("b", "Gaps", "cloze", 0)]);
    fireEvent.click(await screen.findByRole("button", { name: "New note type" }));
    const sheet = await screen.findByRole("dialog", { name: "New note type" });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    expect((await within(sheet).findByRole("alert")).textContent).toContain("name is empty");
    fireEvent.change(within(sheet).getByLabelText("Name"), { target: { value: "Polish" } });
    fireEvent.change(within(sheet).getByLabelText("Start from"), { target: { value: "b" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(window.location.hash).toBe("#/notetypes/new-1"));
    expect(create.mock.calls[0]?.[0]).toEqual({ name: "Polish", startFrom: "b", kind: "standard" });
  });

  test("with none left it offers the Basic and Cloze shapes", async () => {
    const create = setup([]);
    fireEvent.click(await screen.findByRole("button", { name: "New note type" }));
    const sheet = await screen.findByRole("dialog", { name: "New note type" });
    fireEvent.change(within(sheet).getByLabelText("Name"), { target: { value: "Gaps" } });
    fireEvent.change(within(sheet).getByLabelText("Start from"), {
      target: { value: "shape:cloze" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(create).toHaveBeenCalled());
    expect(create.mock.calls[0]?.[0]).toEqual({ name: "Gaps", startFrom: null, kind: "cloze" });
  });
});
