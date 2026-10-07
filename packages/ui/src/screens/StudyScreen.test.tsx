import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, createFakeTransport, type EventRating, type StudyNext } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { CoreProvider } from "../core";
import { PlatformProvider } from "../platform";
import { RouterProvider } from "../router";
import { AppShell } from "../shell/AppShell";
import { THEME_KEY, ThemeProvider } from "../theme";
import { StudyScreen } from "./StudyScreen";

const counts = { newCount: 4, learningCount: 1, reviewCount: 2 };
const previews = [
  { unit: "minutes", amount: 1 },
  { unit: "minutes", amount: 10 },
  { unit: "days", amount: 3 },
  { unit: "days", amount: 76 },
] as const;

function card(id: string, state: "new" | "learning" | "review" = "new"): StudyNext {
  return { kind: "card", cardId: id, deckId: "d1", state, previews: [...previews], counts };
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  window.history.replaceState(null, "", "/#/study/d1");
});
afterEach(cleanup);

type Options = {
  queue: (StudyNext | { fail: string })[];
  summary?: { answered: number; again: number; studiedMs: number; elapsedMs: number } | null;
  undone?: boolean;
};

function setup({ queue, summary = null, undone = true }: Options) {
  const next = [...queue];
  const handlers = {
    startStudySession: vi.fn(() => ({ sessionId: "s1" })),
    endStudySession: vi.fn(() => ({ summary })),
    nextCard: vi.fn(() => {
      const item = next.shift() ?? ({ kind: "done", counts } as StudyNext);
      if ("fail" in item) throw { kind: "internal", message: item.fail };
      return item;
    }),
    renderCard: vi.fn(({ cardId }: { cardId: string }) => ({
      front: `<p>front of ${cardId}</p>`,
      back: `<p>back of ${cardId}</p>`,
      media: ["a.mp3"],
    })),
    answerCard: vi.fn((_input: { cardId: string; rating: EventRating; durationMs: number }) => ({
      eventId: "e1",
    })),
    undoAnswer: vi.fn(() => ({ undone: undone ? { cardId: "c1" } : null })),
    getMedia: vi.fn(() => ({ contentType: "audio/mpeg" })),
  };
  const client = new CoreClient(createFakeTransport(handlers));
  const view = render(
    <PlatformProvider platform={{ cardFrameUrl: "about:blank", setSystemTheme: () => {} }}>
      <ThemeProvider>
        <CoreProvider client={client}>
          <RouterProvider>
            <AppShell>
              <StudyScreen deckId="d1" />
            </AppShell>
          </RouterProvider>
        </CoreProvider>
      </ThemeProvider>
    </PlatformProvider>,
  );
  return { ...view, ...handlers };
}

function frame(): HTMLIFrameElement {
  return document.querySelector("iframe") as HTMLIFrameElement;
}

function press(key: string, init: KeyboardEventInit = {}) {
  fireEvent.keyDown(document.body, { key, ...init });
}

async function showAnswer() {
  fireEvent.click(await screen.findByRole("button", { name: /^Show answer/ }));
}

describe("the question and the answer", () => {
  test("starts a session, shows the first card and what is left", async () => {
    const { startStudySession, nextCard } = setup({ queue: [card("c1")] });
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(startStudySession).toHaveBeenCalledWith({ deckId: "d1" }, expect.anything());
    expect(nextCard).toHaveBeenCalledTimes(1);
    const left = screen.getByRole("list", { name: "Left to study today" });
    expect(left.textContent).toBe("New 4Learn 1Review 2");
    expect(within(left).getByText("New 4").getAttribute("data-active")).toBe("true");
    expect(screen.getByText("Question")).not.toBeNull();
    expect(frame()).not.toBeNull();
    expect(screen.queryByRole("button", { name: /^Good/ })).toBeNull();
  });

  test("Show answer reveals four answers, each with when the card comes back", async () => {
    setup({ queue: [card("c1", "review")] });
    await showAnswer();
    const group = screen.getByRole("group", { name: "How well did you remember it?" });
    const labels = within(group)
      .getAllByRole("button")
      .map((b) => b.textContent?.replace(/[1-4]$/, ""));
    expect(labels).toEqual(["Again1m", "Hard10m", "Good3d", "Easy2.5mo"]);
    expect(screen.getByText("Review 2").getAttribute("data-active")).toBe("true");
    expect(screen.queryByRole("button", { name: /^Show answer/ })).toBeNull();
  });

  test("a relearning card counts under Learn", async () => {
    setup({
      queue: [{ ...(card("c1") as Extract<StudyNext, { kind: "card" }>), state: "relearning" }],
    });
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(screen.getByText("Learn 1").getAttribute("data-active")).toBe("true");
    expect(screen.getByText("New 4").getAttribute("data-active")).toBe("false");
  });
});

describe("answering", () => {
  test("rating records the answer with how long it took, then shows the next card", async () => {
    const { answerCard, renderCard } = setup({ queue: [card("c1"), card("c2")] });
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    await waitFor(() => expect(renderCard).toHaveBeenCalledTimes(2));
    expect(answerCard).toHaveBeenCalledTimes(1);
    expect(answerCard.mock.calls[0]?.[0]).toMatchObject({ cardId: "c1", rating: "good" });
    const duration = answerCard.mock.calls[0]?.[0].durationMs ?? -1;
    expect(duration).toBeGreaterThanOrEqual(0);
    expect(duration).toBeLessThan(5000);
    // The next card starts on its question.
    await screen.findByRole("button", { name: /^Show answer/ });
  });

  test("tapping an answer twice answers once", async () => {
    const { answerCard } = setup({ queue: [card("c1"), card("c2")] });
    await showAnswer();
    const hard = screen.getByRole("button", { name: /^Hard/ });
    fireEvent.click(hard);
    fireEvent.click(hard);
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(answerCard).toHaveBeenCalledTimes(1);
  });

  test("a failed answer says so and nothing moves on", async () => {
    const { answerCard } = setup({ queue: [card("c1")] });
    answerCard.mockImplementation(() => {
      throw { kind: "internal", message: "The answer could not be saved." };
    });
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "The answer could not be saved.",
    );
    expect(screen.getByRole("button", { name: "Try again" })).not.toBeNull();
  });
});

describe("keyboard", () => {
  test("Space shows the answer, 1 to 4 rate, Space again means Good", async () => {
    const { answerCard } = setup({ queue: [card("c1"), card("c2"), card("c3")] });
    await screen.findByRole("button", { name: /^Show answer/ });
    press(" ");
    await screen.findByRole("group", { name: "How well did you remember it?" });
    press("2");
    await screen.findByRole("button", { name: /^Show answer/ });
    press("Enter");
    await screen.findByRole("group", { name: "How well did you remember it?" });
    press(" ");
    await waitFor(() => expect(answerCard).toHaveBeenCalledTimes(2));
    expect(answerCard.mock.calls.map((c) => c[0].rating)).toEqual(["hard", "good"]);
  });

  test("the answer keys do nothing before the answer is shown", async () => {
    const { answerCard } = setup({ queue: [card("c1")] });
    await screen.findByRole("button", { name: /^Show answer/ });
    press("3");
    press("1");
    await Promise.resolve();
    expect(answerCard).not.toHaveBeenCalled();
  });

  test("keys are ignored with a modifier and while typing in a field", async () => {
    const { answerCard } = setup({ queue: [card("c1")] });
    await showAnswer();
    press("3", { metaKey: true });
    press("3", { altKey: true });
    const input = document.createElement("input");
    document.body.append(input);
    fireEvent.keyDown(input, { key: "3" });
    input.remove();
    await Promise.resolve();
    expect(answerCard).not.toHaveBeenCalled();
  });

  test("Enter on a focused button presses that button only", async () => {
    const { answerCard } = setup({ queue: [card("c1"), card("c2")] });
    await showAnswer();
    const hard = screen.getByRole("button", { name: /^Hard/ });
    hard.focus();
    fireEvent.keyDown(hard, { key: "Enter" });
    await Promise.resolve();
    // The key handler leaves it to the button, whose click is what answers.
    expect(answerCard).not.toHaveBeenCalled();
    fireEvent.click(hard);
    await waitFor(() => expect(answerCard).toHaveBeenCalledTimes(1));
  });
});

describe("undo", () => {
  test("is off until something was answered, then brings the card back", async () => {
    const { undoAnswer, nextCard } = setup({ queue: [card("c1"), card("c2"), card("c1")] });
    await showAnswer();
    const undo = () => screen.getByRole("button", { name: /^Undo/ }) as HTMLButtonElement;
    expect(undo().disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(undo().disabled).toBe(false);

    fireEvent.click(undo());
    await waitFor(() => expect(undoAnswer).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(nextCard).toHaveBeenCalledTimes(3));
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(undo().disabled).toBe(true);
  });

  test("z and Ctrl+Z undo too", async () => {
    const { undoAnswer } = setup({ queue: [card("c1"), card("c2"), card("c1"), card("c2")] });
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    await screen.findByRole("button", { name: /^Show answer/ });
    press("z");
    await waitFor(() => expect(undoAnswer).toHaveBeenCalledTimes(1));
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    await screen.findByRole("button", { name: /^Show answer/ });
    press("z", { ctrlKey: true });
    await waitFor(() => expect(undoAnswer).toHaveBeenCalledTimes(2));
  });
});

describe("the end", () => {
  test("ends the session and shows what was done", async () => {
    const { endStudySession } = setup({
      queue: [card("c1")],
      summary: { answered: 3, again: 1, studiedMs: 65_000, elapsedMs: 90_000 },
    });
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    expect(await screen.findByRole("heading", { name: "Done for now" })).not.toBeNull();
    expect(endStudySession).toHaveBeenCalledWith({ sessionId: "s1" }, expect.anything());
    expect(screen.getByText("You answered 3 cards.")).not.toBeNull();
    expect(screen.getByText("1 to see again soon.")).not.toBeNull();
    expect(screen.getByText("Time studied: 1 minute.")).not.toBeNull();
    expect(screen.getByText("Nothing else is due today.")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Back to decks" })).not.toBeNull();
  });

  test("an empty deck says no cards were answered", async () => {
    setup({ queue: [] });
    expect(await screen.findByRole("heading", { name: "Done for now" })).not.toBeNull();
    expect(screen.getByText("No cards were answered this time.")).not.toBeNull();
  });

  test("a learning card due later is waiting, and Check again looks again", async () => {
    const { startStudySession, nextCard } = setup({
      queue: [{ kind: "waiting", waitSeconds: 12 * 60, counts }, card("c1")],
      summary: { answered: 1, again: 0, studiedMs: 4000, elapsedMs: 9000 },
    });
    expect(
      await screen.findByRole("heading", { name: "Nothing to show right now" }),
    ).not.toBeNull();
    expect(screen.getByText("The next card is due in 12 minutes.")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(startStudySession).toHaveBeenCalledTimes(2);
    expect(nextCard).toHaveBeenCalledTimes(2);
  });

  test("the last answer can be undone from the end screen, in a new session", async () => {
    const { startStudySession, undoAnswer } = setup({
      queue: [card("c1"), { kind: "done", counts }, card("c1")],
      summary: { answered: 1, again: 0, studiedMs: 1000, elapsedMs: 2000 },
    });
    await showAnswer();
    fireEvent.click(screen.getByRole("button", { name: /^Good/ }));
    await screen.findByRole("heading", { name: "Done for now" });
    fireEvent.click(screen.getByRole("button", { name: "Undo last answer" }));
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(undoAnswer).toHaveBeenCalledTimes(1);
    expect(startStudySession).toHaveBeenCalledTimes(2);
  });

  test("no undo is offered when nothing was answered here", async () => {
    setup({ queue: [] });
    await screen.findByRole("heading", { name: "Done for now" });
    expect(screen.queryByRole("button", { name: "Undo last answer" })).toBeNull();
  });
});

describe("leaving and failing", () => {
  test("leaving in the middle of a card ends the session and records nothing", async () => {
    const { endStudySession, answerCard, unmount } = setup({ queue: [card("c1")] });
    await showAnswer();
    unmount();
    await waitFor(() =>
      expect(endStudySession).toHaveBeenCalledWith({ sessionId: "s1" }, expect.anything()),
    );
    expect(answerCard).not.toHaveBeenCalled();
  });

  test("a failure to find the next card is shown, and Try again starts over", async () => {
    const { startStudySession } = setup({
      queue: [{ fail: "Could not read your cards." }, card("c1")],
    });
    expect((await screen.findByRole("alert")).textContent).toContain("Could not read your cards.");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(startStudySession).toHaveBeenCalledTimes(2);
  });

  test("a card whose template is broken shows the core's message", async () => {
    const { renderCard } = setup({ queue: [card("c1")] });
    renderCard.mockImplementation(() => {
      throw {
        kind: "invalidInput",
        message: 'The front of the card template "Card 1" has a mistake. Fix the template.',
      };
    });
    expect((await screen.findByRole("alert")).textContent).toContain("has a mistake");
    fireEvent.click(screen.getByRole("link", { name: "Back to decks" }));
    expect(window.location.hash).toBe("#/decks");
  });
});

describe("the card frame", () => {
  async function ready(): Promise<{ message: Record<string, unknown> }> {
    const target = frame().contentWindow as Window;
    const post = vi.spyOn(target, "postMessage");
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { type: "ready" }, source: target }),
      );
    });
    await waitFor(() => expect(post).toHaveBeenCalled());
    return { message: post.mock.calls[0]?.[0] as Record<string, unknown> };
  }

  test("gets the app's theme, autoplay and the card", async () => {
    localStorage.setItem(THEME_KEY, "dark");
    setup({ queue: [card("c1")] });
    await screen.findByRole("button", { name: /^Show answer/ });
    const { message } = await ready();
    expect(message).toMatchObject({ html: "<p>front of c1</p>", theme: "dark", autoplay: true });
  });

  test("the answer side shows the back", async () => {
    setup({ queue: [card("c1")] });
    await showAnswer();
    await screen.findByRole("group", { name: "How well did you remember it?" });
    const { message } = await ready();
    expect(message.html).toBe("<p>back of c1</p>");
  });

  test("Replay sound appears only when the card has sound, and says if autoplay was refused", async () => {
    setup({ queue: [card("c1")] });
    await screen.findByRole("button", { name: /^Show answer/ });
    expect(screen.queryByRole("button", { name: /^Replay sound/ })).toBeNull();
    const target = frame().contentWindow as Window;
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { type: "audio", count: 1 }, source: target }),
      );
    });
    const replay = await screen.findByRole("button", { name: /^Replay sound/ });
    const post = vi.spyOn(target, "postMessage");
    fireEvent.click(replay);
    expect(post).toHaveBeenCalledWith({ type: "play" }, "*");

    expect(screen.queryByText(/did not start by itself/)).toBeNull();
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { type: "autoplay-blocked" }, source: target }),
      );
    });
    expect(await screen.findByText(/did not start by itself/)).not.toBeNull();
  });
});
