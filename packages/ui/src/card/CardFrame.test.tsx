import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { PlatformProvider } from "../platform";
import { CardFrame } from "./CardFrame";

afterEach(cleanup);

function setup(props: Partial<React.ComponentProps<typeof CardFrame>> = {}) {
  const loadMedia = vi.fn(async (name: string) => new Blob([name]));
  const ui = (p: Partial<React.ComponentProps<typeof CardFrame>> = {}) => (
    <PlatformProvider platform={{ cardFrameUrl: "about:blank" }}>
      <CardFrame html="<p>hi</p>" mediaNames={["a.png"]} loadMedia={loadMedia} {...props} {...p} />
    </PlatformProvider>
  );
  const view = render(ui());
  return {
    ...view,
    loadMedia,
    rerenderWith: (p: Parameters<typeof ui>[0]) => view.rerender(ui(p)),
  };
}

function frame(): HTMLIFrameElement {
  return document.querySelector("iframe") as HTMLIFrameElement;
}

/** Delivers a message as if the given window had posted it. */
function messageFrom(source: Window | null, data: unknown) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, source }));
  });
}

test("the iframe is sandboxed with exactly allow-scripts and has no permissions", () => {
  setup();
  const el = frame();
  expect(el.getAttribute("sandbox")).toBe("allow-scripts");
  expect(el.hasAttribute("allow")).toBe(false);
  expect(el.getAttribute("src")).toBe("about:blank");
});

test("posts the card and its media only after ready from its own frame", async () => {
  const { loadMedia } = setup();
  const target = frame().contentWindow as Window;
  const post = vi.spyOn(target, "postMessage");

  await Promise.resolve();
  expect(post).not.toHaveBeenCalled();

  messageFrom(target, { type: "ready" });
  await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
  const [message, targetOrigin] = post.mock.calls[0] as [
    { html: string; media: Record<string, Blob> },
    string,
  ];
  expect(targetOrigin).toBe("*");
  expect(message.html).toBe("<p>hi</p>");
  expect(Object.keys(message.media)).toEqual(["a.png"]);
  expect(loadMedia).toHaveBeenCalledWith("a.png");

  // A second ready from the same frame (a card can fake it) does not send the card again.
  messageFrom(target, { type: "ready" });
  await Promise.resolve();
  expect(post).toHaveBeenCalledTimes(1);
});

test("ignores messages that do not come from its own frame", async () => {
  const { loadMedia } = setup({ onHeight: undefined });
  const target = frame().contentWindow as Window;
  const post = vi.spyOn(target, "postMessage");

  messageFrom(window, { type: "ready" });
  messageFrom(null, { type: "ready" });
  messageFrom(window, { type: "height", px: 500 });
  await Promise.resolve();
  expect(post).not.toHaveBeenCalled();
  expect(loadMedia).not.toHaveBeenCalled();
  expect(frame().style.height).toBe("0px");
});

test("ignores unknown and malformed messages from its frame", async () => {
  setup();
  const target = frame().contentWindow as Window;
  const post = vi.spyOn(target, "postMessage");
  for (const data of ["ready", 42, null, { type: "navigate", url: "x" }, { type: "height" }]) {
    messageFrom(target, data);
  }
  await Promise.resolve();
  expect(post).not.toHaveBeenCalled();
  expect(frame().style.height).toBe("0px");
});

test("clamps the reported height", () => {
  const onHeight = vi.fn();
  setup({ onHeight });
  const target = frame().contentWindow as Window;

  messageFrom(target, { type: "height", px: 240.2 });
  expect(frame().style.height).toBe("241px");
  messageFrom(target, { type: "height", px: 1e9 });
  expect(frame().style.height).toBe("10000px");
  messageFrom(target, { type: "height", px: -50 });
  expect(frame().style.height).toBe("0px");
  messageFrom(target, { type: "height", px: Number.POSITIVE_INFINITY });
  expect(frame().style.height).toBe("0px");
  expect(onHeight.mock.calls.map((c) => c[0])).toEqual([241, 10000, 0, 0]);
});

test("a missing media file does not stop the card", async () => {
  const loadMedia = vi.fn(async () => {
    throw new Error("gone");
  });
  setup({ loadMedia, mediaNames: ["gone.png"] });
  const target = frame().contentWindow as Window;
  const post = vi.spyOn(target, "postMessage");
  messageFrom(target, { type: "ready" });
  await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
  const [message] = post.mock.calls[0] as [{ media: object }];
  expect(message.media).toEqual({});
});

test("removes the frame on the third load and says why", () => {
  setup();
  fireEvent.load(frame());
  fireEvent.load(frame());
  expect(frame()).not.toBeNull();
  fireEvent.load(frame());
  expect(document.querySelector("iframe")).toBeNull();
  expect(screen.getByRole("alert").textContent).toBe(
    "This card tried to open another page and was stopped.",
  );
});

test("new html makes a new iframe", () => {
  const { rerenderWith } = setup();
  const first = frame();
  rerenderWith({ html: "<p>next</p>" });
  expect(frame()).not.toBe(first);
  expect(document.querySelectorAll("iframe").length).toBe(1);
});
