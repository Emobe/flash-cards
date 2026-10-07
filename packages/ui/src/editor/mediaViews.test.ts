import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { loadField, saveField } from "./html";
import { mediaNodeViews } from "./mediaViews";

let urls: string[];
let revoked: string[];
let views: EditorView[];

beforeEach(() => {
  urls = [];
  revoked = [];
  views = [];
  let n = 0;
  URL.createObjectURL = () => {
    const url = `blob:test/${++n}`;
    urls.push(url);
    return url;
  };
  URL.revokeObjectURL = (url: string) => {
    revoked.push(url);
  };
});
afterEach(() => {
  for (const view of views) view.destroy();
  document.body.innerHTML = "";
});

function mount(html: string, load: (name: string) => Promise<Blob>) {
  const host = document.createElement("div");
  document.body.append(host);
  const view = new EditorView(host, {
    state: EditorState.create({ doc: loadField(html) }),
    nodeViews: mediaNodeViews(() => load),
    dispatchTransaction(tr) {
      view.updateState(view.state.apply(tr));
    },
  });
  views.push(view);
  return { view, host };
}

const blob = (type: string) => new Blob(["bytes"], { type });
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("the image view", () => {
  test("shows the picture from a blob URL made from the bytes, not from the field's own name", async () => {
    const load = vi.fn(async () => blob("image/png"));
    const { host } = mount('see <img src="cat-0123456789abcdef.png">', load);
    await settle();
    expect(load).toHaveBeenCalledWith("cat-0123456789abcdef.png");
    const img = host.querySelector("img") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe(urls[0]);
    expect(img.getAttribute("src")).not.toContain("cat-");
    expect(img.alt).toBe("");
  });

  test("the URL is revoked when the view is destroyed", async () => {
    const { view } = mount('<img src="a-0123456789abcdef.png">', async () => blob("image/png"));
    await settle();
    view.destroy();
    views.pop();
    expect(revoked).toEqual([urls[0]]);
  });

  test("a picture that loads after the view is gone makes no URL", async () => {
    let finish: (b: Blob) => void = () => {};
    const { view } = mount(
      '<img src="a-0123456789abcdef.png">',
      () => new Promise<Blob>((resolve) => (finish = resolve)),
    );
    view.destroy();
    views.pop();
    finish(blob("image/png"));
    await settle();
    expect(urls).toEqual([]);
  });

  test("a picture that cannot be read says so and keeps its place in the field", async () => {
    const { host, view } = mount('<img src="gone-0123456789abcdef.png">', async () => {
      throw new Error("not found");
    });
    await settle();
    expect(host.textContent).toContain("Picture missing");
    expect(saveField(view.state.doc)).toBe('<img src="gone-0123456789abcdef.png">');
  });

  test("the remove button deletes it from the field", async () => {
    const { host, view } = mount('a<img src="a-0123456789abcdef.png">b', async () =>
      blob("image/png"),
    );
    (host.querySelector("button[aria-label='Remove picture']") as HTMLButtonElement).click();
    expect(saveField(view.state.doc)).toBe("ab");
  });
});

describe("the sound view", () => {
  test("is a chip with the name and a play button, and fetches nothing until played", () => {
    const load = vi.fn(async () => blob("audio/mpeg"));
    const { host } = mount("[sound:meow-0123456789abcdef.mp3]", load);
    expect(host.textContent).toContain("meow-0123456789abcdef.mp3");
    expect(host.querySelector("button[aria-label='Play sound']")).toBeTruthy();
    expect(load).not.toHaveBeenCalled();
  });

  test("playing it plays an audio element on a blob URL, and the URL is revoked with the view", async () => {
    const played: string[] = [];
    const real = window.Audio;
    window.Audio = class {
      paused = true;
      constructor(public src: string) {}
      addEventListener() {}
      async play() {
        played.push(this.src);
        this.paused = false;
      }
      pause() {
        this.paused = true;
      }
    } as unknown as typeof Audio;
    try {
      const { host, view } = mount("[sound:a-0123456789abcdef.mp3]", async () =>
        blob("audio/mpeg"),
      );
      (host.querySelector("button[aria-label='Play sound']") as HTMLButtonElement).click();
      await settle();
      expect(played).toEqual([urls[0]]);
      view.destroy();
      views.pop();
      expect(revoked).toEqual([urls[0]]);
    } finally {
      window.Audio = real;
    }
  });

  test("a sound that cannot be played says so", async () => {
    const real = window.Audio;
    window.Audio = class {
      paused = true;
      addEventListener() {}
      async play() {
        throw new Error("NotSupportedError");
      }
      pause() {}
    } as unknown as typeof Audio;
    try {
      const { host } = mount("[sound:a-0123456789abcdef.mp3]", async () => blob("audio/mpeg"));
      (host.querySelector("button[aria-label='Play sound']") as HTMLButtonElement).click();
      await settle();
      expect(host.querySelector("[role='status']")?.textContent).toBe(
        "This device cannot play this sound.",
      );
    } finally {
      window.Audio = real;
    }
  });

  test("the remove button deletes it from the field", () => {
    const { host, view } = mount("a[sound:a-0123456789abcdef.mp3]b", async () =>
      blob("audio/mpeg"),
    );
    (host.querySelector("button[aria-label='Remove sound']") as HTMLButtonElement).click();
    expect(saveField(view.state.doc)).toBe("ab");
  });
});
