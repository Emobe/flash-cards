import { act, cleanup, render } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useBackEntry } from "./useBackEntry";

const tick = (ms = 20) => act(() => new Promise((resolve) => setTimeout(resolve, ms)));
const held = () => window.history.state?.fcDialog;

function Holder({ name, onBack }: { name: string; onBack: (name: string) => void }) {
  useBackEntry(true, () => onBack(name));
  return <p>{name}</p>;
}

/** A mode with a window above it; Back closes the window, then the mode. */
function Stacked({ keepWindow = false }: { keepWindow?: boolean }) {
  const [mode, setMode] = useState(true);
  const [window_, setWindow] = useState(true);
  return (
    <>
      {mode && <Holder name="mode" onBack={() => setMode(false)} />}
      {window_ && <Holder name="window" onBack={() => keepWindow || setWindow(false)} />}
      <button type="button" onClick={() => setWindow(false)}>
        close window
      </button>
    </>
  );
}

beforeEach(() => {
  window.history.replaceState(null, "", "/#/browse");
});

afterEach(async () => {
  cleanup();
  await tick();
});

describe("useBackEntry", () => {
  test("each holder has its own entry and Back closes only the top one", async () => {
    const view = render(<Stacked />);
    expect(held()).toBe(2);
    await act(async () => window.history.back());
    await tick();
    expect(view.queryByText("window")).toBeNull();
    expect(view.queryByText("mode")).not.toBeNull();
    expect(held()).toBe(1);
    await act(async () => window.history.back());
    await tick();
    expect(view.queryByText("mode")).toBeNull();
    expect(held()).toBeUndefined();
  });

  test("a holder that stays open gets its entry back", async () => {
    const view = render(<Stacked keepWindow />);
    await act(async () => window.history.back());
    await tick();
    expect(view.queryByText("window")).not.toBeNull();
    expect(held()).toBe(2);
  });

  test("closing by a button takes the entry back", async () => {
    const view = render(<Stacked />);
    const before = window.history.length;
    act(() => view.getByText("close window").click());
    await tick();
    expect(held()).toBe(1);
    expect(window.history.length).toBe(before);
  });

  test("an entry left with no holder is skipped by Back", async () => {
    const onBack = vi.fn();
    const view = render(<Holder name="one" onBack={onBack} />);
    // A route change under the holder: its entry stays behind the new one.
    window.history.pushState(null, "", "/#/decks");
    view.unmount();
    await tick();
    await act(async () => window.history.back());
    await tick(50);
    expect(window.location.hash).toBe("#/browse");
    expect(held()).toBeUndefined();
    expect(onBack).not.toHaveBeenCalled();
  });
});
