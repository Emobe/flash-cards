import type { Node } from "prosemirror-model";
import type { EditorView, NodeView } from "prosemirror-view";

/** Fetches a stored file's bytes, for showing it. The screen supplies it (`getMedia`, or a cache). */
export type LoadMedia = (name: string) => Promise<Blob>;

type GetPos = () => number | undefined;

function removeButton(label: string, view: EditorView, getPos: GetPos, node: Node) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "pm-remove";
  button.setAttribute("aria-label", label);
  button.textContent = "×";
  button.addEventListener("click", () => {
    const at = getPos();
    if (at === undefined) return;
    view.dispatch(view.state.tr.delete(at, at + node.nodeSize));
    view.focus();
  });
  return button;
}

/**
 * The picture is shown from a `blob:` URL that this code made from bytes it was given (ADR 0011
 * decision 1). The field's own `src` is a media name and is never put in the page. The URL is
 * revoked when the view goes away.
 */
class ImageView implements NodeView {
  dom: HTMLElement;
  private image = document.createElement("img");
  private url: string | null = null;
  private gone = false;

  constructor(
    private node: Node,
    view: EditorView,
    getPos: GetPos,
    load: LoadMedia,
  ) {
    this.dom = document.createElement("span");
    this.dom.className = "pm-image";
    this.image.alt = "";
    this.image.className = "pm-picture";
    this.dom.append(this.image, removeButton("Remove picture", view, getPos, node));
    const name = String(node.attrs.name);
    this.dom.title = name;
    load(name)
      .then((blob) => {
        if (this.gone) return;
        this.url = URL.createObjectURL(blob);
        this.image.src = this.url;
      })
      .catch(() => {
        if (this.gone) return;
        this.dom.classList.add("pm-missing");
        this.image.replaceWith(
          Object.assign(document.createElement("span"), { textContent: "Picture missing" }),
        );
      });
  }

  update(node: Node) {
    return node.type === this.node.type && node.attrs.name === this.node.attrs.name;
  }

  // Clicks on the remove button are ours, not the editor's.
  stopEvent(event: Event) {
    return event.target instanceof HTMLButtonElement;
  }

  ignoreMutation() {
    return true;
  }

  destroy() {
    this.gone = true;
    if (this.url) URL.revokeObjectURL(this.url);
  }
}

/** A chip with the file name and a play button. The `<audio>` element is made on the first play. */
class SoundView implements NodeView {
  dom: HTMLElement;
  private play = document.createElement("button");
  private status = document.createElement("span");
  private audio: HTMLAudioElement | null = null;
  private url: string | null = null;
  private gone = false;

  constructor(
    private node: Node,
    view: EditorView,
    getPos: GetPos,
    private load: LoadMedia,
  ) {
    const name = String(node.attrs.name);
    this.dom = document.createElement("span");
    this.dom.className = "pm-sound";
    this.dom.title = name;
    this.play.type = "button";
    this.play.className = "pm-play";
    this.setPlaying(false);
    this.play.addEventListener("click", () => void this.toggle());
    const label = document.createElement("span");
    label.className = "pm-sound-name";
    label.textContent = name;
    this.status.className = "visually-hidden";
    this.status.setAttribute("role", "status");
    this.dom.append(
      this.play,
      label,
      this.status,
      removeButton("Remove sound", view, getPos, node),
    );
  }

  private setPlaying(playing: boolean) {
    this.play.textContent = playing ? "⏸" : "▶";
    this.play.setAttribute("aria-label", playing ? "Pause sound" : "Play sound");
  }

  private async toggle() {
    try {
      if (!this.audio) {
        const blob = await this.load(String(this.node.attrs.name));
        if (this.gone) return;
        this.url = URL.createObjectURL(blob);
        this.audio = new Audio(this.url);
        this.audio.addEventListener("ended", () => this.setPlaying(false));
        this.audio.addEventListener("pause", () => this.setPlaying(false));
        this.audio.addEventListener("play", () => this.setPlaying(true));
      }
      if (this.audio.paused) await this.audio.play();
      else this.audio.pause();
      this.status.textContent = "";
    } catch {
      this.setPlaying(false);
      this.status.textContent = "This device cannot play this sound.";
      this.play.title = "This device cannot play this sound.";
    }
  }

  update(node: Node) {
    return node.type === this.node.type && node.attrs.name === this.node.attrs.name;
  }

  stopEvent(event: Event) {
    return event.target instanceof HTMLButtonElement;
  }

  ignoreMutation() {
    return true;
  }

  destroy() {
    this.gone = true;
    this.audio?.pause();
    if (this.url) URL.revokeObjectURL(this.url);
  }
}

/** The node views for the schema's `image` and `sound` nodes. */
export function mediaNodeViews(load: () => LoadMedia) {
  return {
    image: (node: Node, view: EditorView, getPos: GetPos) =>
      new ImageView(node, view, getPos, (name) => load()(name)),
    sound: (node: Node, view: EditorView, getPos: GetPos) =>
      new SoundView(node, view, getPos, (name) => load()(name)),
  };
}
