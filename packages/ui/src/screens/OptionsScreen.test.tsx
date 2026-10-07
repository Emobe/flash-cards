import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  CoreClient,
  createFakeTransport,
  type DeckList,
  type DeckSummary,
  type PresetSummary,
} from "core-client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { CoreProvider } from "../core";
import { RouterProvider } from "../router";
import { OptionsScreen, PRESET_HELP, parseSteps } from "./OptionsScreen";

function preset(id: string, name: string, extra: Partial<PresetSummary> = {}): PresetSummary {
  return {
    id,
    name,
    isDefault: id === "p-default",
    newPerDay: 20,
    reviewsPerDay: 200,
    learningSteps: [1, 10],
    relearningSteps: [10],
    desiredRetention: 0.9,
    spaceSiblings: true,
    deckCount: 1,
    ...extra,
  };
}

function deck(id: string, name: string, presetId: string): DeckSummary {
  return {
    id,
    name,
    path: name,
    parentId: null,
    depth: 0,
    isDefault: false,
    presetId,
    newCount: 0,
    learningCount: 0,
    reviewCount: 0,
  };
}

beforeEach(() => window.history.replaceState(null, "", "/#/options/d-polish"));
afterEach(cleanup);

function renderOptions(
  state: { presets: PresetSummary[]; deckPreset: string },
  overrides: Record<string, (input: never) => unknown> = {},
) {
  const calls: { method: string; input: unknown }[] = [];
  const record = (method: string, result: unknown) => (input: never) => {
    calls.push({ method, input });
    return overrides[method] ? overrides[method](input) : result;
  };
  const client = new CoreClient(
    createFakeTransport({
      getDeckList: () =>
        ({
          totalCards: 1,
          decks: [deck("d-polish", "Polish", state.deckPreset)],
        }) satisfies DeckList,
      getPresets: () => ({ presets: state.presets }),
      setDeckPreset: record("setDeckPreset", null),
      createPreset: record("createPreset", { id: "p-new" }),
      renamePreset: record("renamePreset", null),
      deletePreset: record("deletePreset", { decks: 1 }),
      setPresetOptions: record("setPresetOptions", null),
    } as never),
  );
  render(
    <CoreProvider client={client}>
      <RouterProvider>
        <OptionsScreen deckId="d-polish" />
      </RouterProvider>
    </CoreProvider>,
  );
  return calls;
}

const basic = {
  presets: [preset("p-default", "Default", { deckCount: 0 }), preset("p-hard", "Hard words")],
  deckPreset: "p-hard",
};

describe("parseSteps", () => {
  test("reads minutes separated by spaces or commas", () => {
    expect(parseSteps("1 10")).toEqual([1, 10]);
    expect(parseSteps(" 1,  10 ,60 ")).toEqual([1, 10, 60]);
    expect(parseSteps("")).toEqual([]);
  });
  test("says what is wrong", () => {
    expect(parseSteps("1 x")).toMatch(/whole numbers/);
    expect(parseSteps("0")).toMatch(/1 to 1440/);
    expect(parseSteps("1441")).toMatch(/1 to 1440/);
    expect(parseSteps("1 2 3 4 5 6 7 8 9")).toMatch(/at most 8/);
    expect(parseSteps("1.5")).toMatch(/whole numbers/);
  });
});

describe("Deck options", () => {
  test("shows the deck's preset, its settings and a plain explanation for every one", async () => {
    renderOptions(basic);
    expect(await screen.findByRole("heading", { name: "Options: Polish" })).toBeDefined();
    expect((screen.getByLabelText("This deck uses") as HTMLSelectElement).value).toBe("p-hard");
    expect((screen.getByLabelText("New cards per day") as HTMLInputElement).value).toBe("20");
    expect((screen.getByLabelText("Learning steps (minutes)") as HTMLInputElement).value).toBe(
      "1 10",
    );
    expect(screen.getByLabelText("Desired retention: 90%")).toBeDefined();
    // Every field is described by its own help text.
    for (const help of Object.values(PRESET_HELP)) {
      expect(screen.getByText(help)).toBeDefined();
    }
    const field = screen.getByLabelText("Maximum reviews per day");
    const described = document.getElementById(field.getAttribute("aria-describedby") ?? "");
    expect(described?.textContent).toBe(PRESET_HELP.reviewsPerDay);
  });

  test("choosing another preset gives it to the deck", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("This deck uses");
    fireEvent.change(screen.getByLabelText("This deck uses"), { target: { value: "p-default" } });
    await waitFor(() =>
      expect(calls).toEqual([
        { method: "setDeckPreset", input: { deckId: "d-polish", presetId: "p-default" } },
      ]),
    );
  });

  test("Save sends every setting, with retention as a fraction", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("New cards per day");
    fireEvent.change(screen.getByLabelText("New cards per day"), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText("Learning steps (minutes)"), {
      target: { value: "2, 20 60" },
    });
    fireEvent.change(screen.getByLabelText("Relearning steps (minutes)"), {
      target: { value: "" },
    });
    fireEvent.change(screen.getByLabelText("Desired retention: 90%"), { target: { value: "95" } });
    fireEvent.click(screen.getByLabelText("Delay sibling cards"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved.")).toBeDefined();
    expect(calls).toEqual([
      {
        method: "setPresetOptions",
        input: {
          presetId: "p-hard",
          newPerDay: 5,
          reviewsPerDay: 200,
          learningSteps: [2, 20, 60],
          relearningSteps: [],
          desiredRetention: 0.95,
          spaceSiblings: false,
        },
      },
    ]);
  });

  test("a bad value is explained next to its field and nothing is sent", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("New cards per day");
    fireEvent.change(screen.getByLabelText("New cards per day"), { target: { value: "ten" } });
    fireEvent.change(screen.getByLabelText("Learning steps (minutes)"), { target: { value: "0" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const field = screen.getByLabelText("New cards per day");
    expect(field.getAttribute("aria-invalid")).toBe("true");
    expect(
      document.getElementById(`${field.getAttribute("aria-describedby")?.split(" ")[1]}`)
        ?.textContent,
    ).toBe("Use a whole number, like 20.");
    expect(screen.getByText("Each step is from 1 to 1440 minutes.")).toBeDefined();
    expect(calls).toEqual([]);
  });

  test("a refusal from the core is shown and the form keeps what was typed", async () => {
    renderOptions(basic, {
      setPresetOptions: () => {
        throw { kind: "invalidInput", message: "A daily limit can be at most 9999." };
      },
    });
    await screen.findByLabelText("New cards per day");
    fireEvent.change(screen.getByLabelText("New cards per day"), { target: { value: "50" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "A daily limit can be at most 9999.",
    );
    expect((screen.getByLabelText("New cards per day") as HTMLInputElement).value).toBe("50");
  });

  test("New preset creates it and gives it to the deck", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("This deck uses");
    fireEvent.click(screen.getByRole("button", { name: "New preset" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Verbs" } });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toEqual([
      { method: "createPreset", input: { name: "Verbs" } },
      { method: "setDeckPreset", input: { deckId: "d-polish", presetId: "p-new" } },
    ]);
  });

  test("Rename starts with the current name", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("This deck uses");
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Hard words");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Hard" } });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Rename" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toEqual([
      { method: "renamePreset", input: { presetId: "p-hard", name: "Hard" } },
    ]);
  });

  test("the Default preset cannot be deleted; another asks first and says where decks go", async () => {
    const calls = renderOptions(basic);
    await screen.findByLabelText("This deck uses");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog").textContent).toContain(
      "The deck that uses it will use the Default preset instead.",
    );
    expect(calls).toEqual([]);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    expect(
      await screen.findByText(/Deleted "Hard words"\. 1 deck uses the Default preset now\./),
    ).toBeDefined();
    expect(calls).toEqual([{ method: "deletePreset", input: { presetId: "p-hard" } }]);
  });

  test("the Default preset has no Delete button", async () => {
    renderOptions({ presets: basic.presets, deckPreset: "p-default" });
    await screen.findByLabelText("This deck uses");
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });

  test("a deck that is gone says so", async () => {
    window.history.replaceState(null, "", "/#/options/d-gone");
    const client = new CoreClient(
      createFakeTransport({
        getDeckList: () => ({ totalCards: 0, decks: [] }),
        getPresets: () => ({ presets: [] }),
      } as never),
    );
    render(
      <CoreProvider client={client}>
        <RouterProvider>
          <OptionsScreen deckId="d-gone" />
        </RouterProvider>
      </CoreProvider>,
    );
    expect((await screen.findByRole("alert")).textContent).toContain("That deck was deleted");
    await act(async () => {});
  });
});
