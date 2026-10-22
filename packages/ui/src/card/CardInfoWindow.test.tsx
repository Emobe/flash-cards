import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { type CardInfoEntry, CoreClient, createFakeTransport } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { CardInfoWindow } from "./CardInfoWindow";
import { describeSchedule, entryDetail, formatMoment } from "./cardInfo";
import { infoNew, infoReview } from "./testInfo";

beforeEach(() => {
  window.history.replaceState(null, "", "/#/browse");
});
afterEach(cleanup);

function setup(getCardInfo: () => unknown) {
  const handler = vi.fn((_input: { cardId: string }) => getCardInfo() as never);
  const onClose = vi.fn();
  const client = new CoreClient(createFakeTransport({ getCardInfo: handler }));
  render(
    <CoreProvider client={client}>
      <CardInfoWindow cardId="c-kot" onClose={onClose} />
    </CoreProvider>,
  );
  return { handler, onClose };
}

describe("the card info window", () => {
  test("shows the schedule, the note and the history, newest first", async () => {
    const { handler } = setup(() => infoReview);
    const window = await screen.findByRole("dialog", { name: "Card info" });
    await within(window).findByText("Review: due in 3 days");
    expect(handler).toHaveBeenCalledWith({ cardId: "c-kot" }, expect.anything());
    expect(within(window).getByText("12.3 days")).not.toBeNull();
    expect(within(window).getByText("5.7 of 10")).not.toBeNull();
    expect(within(window).getByText("2 days ago")).not.toBeNull();
    expect(within(window).getByText("Polish::Verbs")).not.toBeNull();
    expect(within(window).getByText("animals, nouns")).not.toBeNull();
    const entries = within(window).getAllByRole("listitem");
    expect(entries.map((e) => within(e).getByRole("strong", {}).textContent)).toEqual([
      "Due date set by hand",
      "Easy",
      "Good",
    ]);
  });

  test("a field is text, never markup, and an empty one says so", async () => {
    setup(() => infoReview);
    const window = await screen.findByRole("dialog", { name: "Card info" });
    await within(window).findByText("kot <b>not bold</b>");
    expect(window.querySelector("b")).toBeNull();
    expect(within(window).getByText("(empty)")).not.toBeNull();
  });

  test("a new card has no history and no memory", async () => {
    setup(() => infoNew);
    const window = await screen.findByRole("dialog", { name: "Card info" });
    await within(window).findByText("New");
    expect(within(window).getByText("Not answered yet.")).not.toBeNull();
    expect(within(window).queryByText("Stability")).toBeNull();
    expect(within(window).queryByText("Last answered")).toBeNull();
  });

  test("a failure says what happened and Try again reads it again", async () => {
    let fail = true;
    const { handler } = setup(() => {
      if (fail) throw { kind: "notFound", message: "That card was deleted." };
      return infoNew;
    });
    expect((await screen.findByRole("alert")).textContent).toContain("That card was deleted.");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(handler).toHaveBeenCalledTimes(2);
  });

  test("Close and Back both close it", async () => {
    const { onClose } = setup(() => infoNew);
    const window = await screen.findByRole("dialog", { name: "Card info" });
    fireEvent.click(within(window).getByRole("button", { name: "Close" }));
    fireEvent.click(within(window).getByRole("button", { name: "Back" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

describe("card info text", () => {
  test("says why a card is not being studied before when it is due", () => {
    const base = infoReview.card;
    expect(describeSchedule({ ...base, suspended: true })).toBe("Review: suspended");
    expect(describeSchedule({ ...base, buried: true })).toBe("Review: buried until tomorrow");
    expect(describeSchedule({ ...base, state: "new", due: null })).toBe("New");
    expect(
      describeSchedule({ ...base, state: "relearning", due: { kind: "time", minutes: 5 } }),
    ).toBe("Relearning: in 5 minutes");
  });

  test("a moment is shown in the local time of the device that made it", () => {
    const at = Date.UTC(2026, 9, 1, 23, 30);
    expect(formatMoment(at, 0)).toContain("23:30");
    expect(formatMoment(at, 120)).toContain("01:30");
    expect(formatMoment(at, 120)).toContain("2");
  });

  test("an entry's detail says what it did, and a hand-set date is not an answer", () => {
    const [first, second, third] = infoReview.history as [
      CardInfoEntry,
      CardInfoEntry,
      CardInfoEntry,
    ];
    expect(entryDetail(first)).toBe("New to learning · back in 10 minutes · took 4 seconds");
    expect(entryDetail(second)).toBe(
      "0 days after the last answer · Learning to review · back in 4 days · took 2 seconds",
    );
    expect(entryDetail(third)).toBe("Review · back in 5 days");
  });
});
