import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type NoteTypeDetails,
  type NoteTypeDraft,
  type PreviewCardInput,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../../core";
import { PlatformProvider } from "../../platform";
import { RouterProvider } from "../../router";
import { AppShell } from "../../shell/AppShell";
import { ThemeProvider } from "../../theme";
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

const rendered = (front = "<p>front</p>") => ({
  front,
  back: "<p>back</p>",
  media: [],
  problems: [],
  unknownFields: [],
  makesCard: true,
});

/** Which queries match: "wide" is 600 px and up, "two" is 840 px and up. */
function widths(wide: boolean, two = false) {
  window.matchMedia = ((query: string) => ({
    matches: (wide && query === "(min-width: 600px)") || (two && query === "(min-width: 840px)"),
    addEventListener() {},
    removeEventListener() {},
  })) as unknown as typeof window.matchMedia;
}

beforeEach(() => widths(false));

function setup(
  handlers: Record<string, (input: never) => unknown> = {},
  loaded: NoteTypeDetails = details(),
) {
  const calls = {
    getNoteTypeDetails: vi.fn(() => loaded),
    previewNotes: vi.fn((_: unknown) => ({
      notes: [
        { noteId: "n2", title: "kot" },
        { noteId: "n1", title: "pies" },
      ],
      total: 2,
    })),
    previewCard: vi.fn((_: PreviewCardInput) => rendered()),
    getNote: vi.fn((_: unknown) => ({
      noteTypeId: "nt",
      fields: [{ fieldId: "f1", value: "{{c1::a}} {{c3::b}}" }],
      tags: [],
      deckIds: [],
      cardCount: 2,
    })),
    checkNoteType: vi.fn((_: unknown) => ({ cardsAdded: 0, cardsRemoved: 0, notesAffected: 0 })),
    saveNoteType: vi.fn((_: unknown) => ({
      message: "Saved Polish words.",
      cardsAdded: 0,
      cardsRemoved: 0,
      undo: { id: "u1", message: "Saved Polish words." },
    })),
    ...handlers,
  };
  const client = new CoreClient(createFakeTransport(calls as never));
  render(
    <PlatformProvider platform={{ cardFrameUrl: "about:blank", setSystemTheme: () => {} }}>
      <ThemeProvider>
        <CoreProvider client={client}>
          <RouterProvider>
            <AppShell>
              <NoteTypeScreen noteTypeId="nt" />
            </AppShell>
          </RouterProvider>
        </CoreProvider>
      </ThemeProvider>
    </PlatformProvider>,
  );
  return calls;
}

async function openEditor() {
  await screen.findByRole("textbox", { name: "Name" });
  const direct = screen.queryByRole("button", { name: "Edit template Card 1" });
  if (direct) fireEvent.click(direct);
  else {
    fireEvent.click(screen.getByRole("button", { name: "Options for Card 1" }));
    const sheet = await screen.findByRole("dialog", { name: "Card 1" });
    fireEvent.click(within(sheet).getByRole("button", { name: "Edit template" }));
  }
  return (await screen.findByRole("textbox", { name: "Front template" })) as HTMLTextAreaElement;
}

const lastInput = (calls: { previewCard: { mock: { calls: unknown[][] } } }) =>
  calls.previewCard.mock.calls.at(-1)?.[0] as PreviewCardInput;

const lastDraft = (calls: { previewCard: { mock: { calls: unknown[][] } } }) => {
  return lastInput(calls).draft;
};

describe("the template box", () => {
  test("opens on the card's front with the keyboard helpers off", async () => {
    setup();
    const box = await openEditor();
    expect(box.value).toBe("{{Front}}");
    expect(box.getAttribute("autocapitalize")).toBe("off");
    expect(box.getAttribute("autocorrect")).toBe("off");
    expect(box.getAttribute("spellcheck")).toBe("false");
  });

  test("an Insert button puts the field at the cursor", async () => {
    setup();
    const box = await openEditor();
    box.setSelectionRange(0, 0);
    fireEvent.click(screen.getByRole("button", { name: "Insert Back" }));
    await waitFor(() => expect(box.value).toBe("{{Back}}{{Front}}"));
    expect(box.selectionStart).toBe(8);
  });

  test("Show if... wraps the selection", async () => {
    setup();
    const box = await openEditor();
    box.setSelectionRange(0, 9);
    fireEvent.click(screen.getByRole("button", { name: "Show if…" }));
    const sheet = await screen.findByRole("dialog", { name: "Show only if a field has text" });
    fireEvent.click(within(sheet).getByRole("button", { name: "Back" }));
    await waitFor(() => expect(box.value).toBe("{{#Back}}{{Front}}{{/Back}}"));
  });

  test("the Back tab offers FrontSide, and a cloze type offers cloze for each field", async () => {
    setup({}, details({ kind: "cloze" }));
    await openEditor();
    expect(screen.getByRole("button", { name: "Insert cloze Front" })).toBeDefined();
    expect(screen.queryByRole("button", { name: "FrontSide" })).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Back" }));
    await screen.findByRole("textbox", { name: "Back template" });
    expect(screen.getByRole("button", { name: "FrontSide" })).toBeDefined();
  });

  test("Done keeps the edit in the draft, and Save sends it", async () => {
    const calls = setup();
    const box = await openEditor();
    fireEvent.change(box, { target: { value: "{{Back}}" } });
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "Front template" })).toBeNull(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.saveNoteType).toHaveBeenCalled());
    const sent = calls.saveNoteType.mock.calls[0]?.[0] as { draft: NoteTypeDraft };
    expect(sent.draft.templates[0]?.front).toBe("{{Back}}");
  });

  test("Styling edits the note type's CSS", async () => {
    const calls = setup();
    await screen.findByRole("textbox", { name: "Name" });
    fireEvent.click(screen.getByRole("button", { name: "Styling" }));
    const box = (await screen.findByRole("textbox", {
      name: "Styling template",
    })) as HTMLTextAreaElement;
    expect(box.value).toBe(".card {}");
    fireEvent.change(box, { target: { value: ".card { color: red }" } });
    await waitFor(() => expect(lastDraft(calls).css).toBe(".card { color: red }"));
  });
});

describe("the preview", () => {
  // Both panes are visible from 840 px.
  beforeEach(() => widths(true, true));

  test("shows the draft's card for the newest note, in a card frame", async () => {
    const calls = setup();
    await openEditor();
    await waitFor(() => expect(calls.previewCard).toHaveBeenCalled());
    const first = calls.previewCard.mock.calls[0]?.[0] as PreviewCardInput;
    expect(first.noteId).toBe("n2");
    expect(first.templateIndex).toBe(0);
    await waitFor(() => expect(document.querySelectorAll("iframe[title='Card']").length).toBe(1));
  });

  test("waits for a pause in typing, then asks with the new text", async () => {
    const calls = setup();
    const box = await openEditor();
    await waitFor(() => expect(calls.previewCard).toHaveBeenCalledTimes(1));
    fireEvent.change(box, { target: { value: "{{Back}}" } });
    fireEvent.change(box, { target: { value: "{{Back}} x" } });
    expect(calls.previewCard).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(calls.previewCard).toHaveBeenCalledTimes(2), { timeout: 2000 });
    expect(lastDraft(calls).templates[0]?.front).toBe("{{Back}} x");
  });

  test("a mistake is listed, keeps the last preview dimmed, and tapping it moves the cursor", async () => {
    let broken = false;
    setup({
      previewCard: () =>
        broken
          ? {
              front: null,
              back: null,
              media: [],
              unknownFields: [],
              makesCard: false,
              problems: [
                {
                  side: "front",
                  mistakes: [{ line: 2, column: 3, message: "This tag is not closed." }],
                },
              ],
            }
          : rendered(),
    });
    const box = await openEditor();
    await waitFor(() => expect(document.querySelector("iframe")).not.toBeNull());
    broken = true;
    fireEvent.change(box, { target: { value: "ab\ncd{{#" } });
    const mistake = await screen.findByRole(
      "button",
      { name: /Front, line 2, column 3: This tag is not closed\./ },
      { timeout: 2000 },
    );
    expect(screen.getByText("Fix the mistake to update the preview.")).toBeDefined();
    // The last good card is still there, dimmed.
    expect(document.querySelector("iframe")).not.toBeNull();
    expect(document.querySelector(".preview-card")?.getAttribute("data-stale")).toBe("true");
    fireEvent.click(mistake);
    await waitFor(() => expect(box.selectionStart).toBe(5));
  });

  test("a field the template reads that is not a field is a warning", async () => {
    setup({ previewCard: () => ({ ...rendered(), unknownFields: ["Meaning"] }) });
    await openEditor();
    await waitFor(() => expect(document.querySelectorAll(".problem-warning").length).toBe(1));
    expect(document.querySelector(".problem-warning")?.textContent).toBe(
      "This template uses Meaning, which is not a field. It will show nothing.",
    );
  });

  test("Previous and Next step through real notes", async () => {
    const calls = setup();
    await openEditor();
    await waitFor(() => expect(calls.previewCard).toHaveBeenCalled());
    fireEvent.click(await screen.findByRole("button", { name: "Next" }));
    await waitFor(() => expect(lastInput(calls).noteId).toBe("n1"));
    expect(screen.getByText(/pies \(2 of 2\)/)).toBeDefined();
  });

  test("Front and Back switch the side shown", async () => {
    setup();
    await openEditor();
    const back = await screen.findByRole("button", { name: "Back", pressed: false });
    fireEvent.click(back);
    expect(screen.getByRole("button", { name: "Back", pressed: true })).toBeDefined();
  });

  test("with no notes it says it is example text", async () => {
    const calls = setup({ previewNotes: () => ({ notes: [], total: 0 }) });
    await openEditor();
    await screen.findByText("Example text, because this note type has no notes yet.");
    await waitFor(() => expect(calls.previewCard).toHaveBeenCalled());
    const first = calls.previewCard.mock.calls[0]?.[0] as PreviewCardInput;
    expect(first.noteId).toBeNull();
  });

  test("a note that makes no card says so", async () => {
    setup({ previewCard: () => ({ ...rendered(), makesCard: false }) });
    await openEditor();
    await screen.findByText("This note makes no card with this template.");
  });

  test("a cloze note lists its numbers, and choosing one asks for it", async () => {
    const calls = setup({}, details({ kind: "cloze" }));
    await openEditor();
    const picker = (await screen.findByRole("combobox", {
      name: "Cloze number",
    })) as HTMLSelectElement;
    expect([...picker.options].map((o) => o.value)).toEqual(["1", "3"]);
    fireEvent.change(picker, { target: { value: "3" } });
    await waitFor(() => expect(lastInput(calls).ordinal).toBe(3));
  });
});

describe("the layouts", () => {
  test("on a phone, Preview closes the keyboard and Edit brings the cursor back", async () => {
    setup();
    const box = await openEditor();
    box.focus();
    box.setSelectionRange(3, 3);
    fireEvent.select(box);
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(box.hidden || box.closest("[hidden]") !== null).toBe(true);
    expect(document.activeElement).not.toBe(box);
    expect(screen.getByRole("region", { name: "Preview" }).closest("[hidden]")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    await waitFor(() => expect(document.activeElement).toBe(box));
    expect(box.selectionStart).toBe(3);
  });

  test("from 840 px the text and the preview are side by side, with no switch", async () => {
    widths(true, true);
    setup();
    const box = await openEditor();
    expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
    expect(box.closest("[hidden]")).toBeNull();
    expect(screen.getByRole("region", { name: "Preview" }).closest("[hidden]")).toBeNull();
    expect(box.getAttribute("wrap")).toBe("off");
  });
});
