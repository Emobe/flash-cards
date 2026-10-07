import { type Ref, useEffect, useImperativeHandle, useRef, useState } from "react";
import { usePlatform } from "../platform";

/** Taller than any real card. A card cannot make the app lay out more than this. */
const MAX_HEIGHT_PX = 10_000;

/** The frame page makes two `load` events by itself (ADR 0005, finding 8). A third is a navigation. */
const LOADS_BEFORE_NAVIGATION = 3;

/** What a parent can ask of a shown card. */
export type CardFrameHandle = {
  /** Plays the card's audio from the start. A card without audio ignores it. */
  play(): void;
};

/** The most audio elements the frame may report, so a card cannot make the count absurd. */
const MAX_AUDIO = 1000;

type Props = {
  /** The finished card document. It is untrusted. */
  html: string;
  /** Media files the card refers to by name. Only these reach the card. */
  mediaNames: string[];
  loadMedia: (name: string) => Promise<Blob>;
  /** Called with each height the card reports, clamped. For layout checks and timing. */
  onHeight?: (px: number) => void;
  /** The app's effective theme. The card follows it, not the system (ADR 0005 amendment, 2.3). */
  theme?: "light" | "dark";
  /** Start the card's audio as soon as it is shown. */
  autoplay?: boolean;
  /** Called once with how many `<audio>` elements the card contains. */
  onAudio?: (count: number) => void;
  /** Called when the browser refused to start audio without a tap. */
  onAutoplayBlocked?: () => void;
  ref?: Ref<CardFrameHandle>;
};

/**
 * The only way card content is shown (ADR 0005). It renders `html` in a sandboxed iframe with an
 * opaque origin, so the card cannot reach the app, its storage or its core. A new `html` or media
 * list gives a new frame.
 */
export function CardFrame(props: Props) {
  const theme = props.theme ?? "light";
  return (
    <CardFrameInstance
      {...props}
      theme={theme}
      key={`${theme}\0${props.mediaNames.join("\0")}\0\0${props.html}`}
    />
  );
}

function CardFrameInstance(props: Props) {
  const { ref } = props;
  const { cardFrameUrl } = usePlatform();
  const frameRef = useRef<HTMLIFrameElement>(null);
  const loads = useRef(0);
  const [height, setHeight] = useState(0);
  const [navigated, setNavigated] = useState(false);

  // The props are read once per frame, when it announces itself. A change makes a new instance.
  const latest = useRef(props);
  latest.current = props;

  useImperativeHandle(ref, () => ({
    play() {
      // Only the parent can send this, and the frame only believes its parent.
      frameRef.current?.contentWindow?.postMessage({ type: "play" }, "*");
    },
  }));

  useEffect(() => {
    let sent = false;
    let alive = true;

    async function sendCard(target: Window) {
      const { html, mediaNames, loadMedia, theme, autoplay } = latest.current;
      const media: Record<string, Blob> = {};
      await Promise.all(
        mediaNames.map(async (name) => {
          try {
            media[name] = await loadMedia(name);
          } catch {
            // A missing file leaves a broken image in the card, not a broken card.
          }
        }),
      );
      // The frame has an opaque origin, which cannot be named as a target origin.
      if (alive) target.postMessage({ html, media, theme, autoplay: autoplay === true }, "*");
    }

    function onMessage(event: MessageEvent) {
      // Messages from a card are untrusted. Only this frame's own window counts, and only these two
      // types. A card can fake them, which affects nothing outside its own frame.
      const target = frameRef.current?.contentWindow;
      if (!target || event.source !== target) return;
      const data: unknown = event.data;
      if (typeof data !== "object" || data === null) return;
      const message = data as { type?: unknown; px?: unknown; count?: unknown };
      if (message.type === "ready" && !sent) {
        sent = true;
        void sendCard(target);
      } else if (message.type === "height" && typeof message.px === "number") {
        const px = Number.isFinite(message.px)
          ? Math.min(Math.max(Math.ceil(message.px), 0), MAX_HEIGHT_PX)
          : 0;
        setHeight(px);
        latest.current.onHeight?.(px);
      } else if (message.type === "audio" && typeof message.count === "number") {
        const count = Number.isFinite(message.count)
          ? Math.min(Math.max(Math.floor(message.count), 0), MAX_AUDIO)
          : 0;
        latest.current.onAudio?.(count);
      } else if (message.type === "autoplay-blocked") {
        latest.current.onAutoplayBlocked?.();
      }
    }

    function onLoad() {
      loads.current += 1;
      if (loads.current >= LOADS_BEFORE_NAVIGATION) setNavigated(true);
    }

    const element = frameRef.current;
    window.addEventListener("message", onMessage);
    element?.addEventListener("load", onLoad);
    return () => {
      alive = false;
      window.removeEventListener("message", onMessage);
      element?.removeEventListener("load", onLoad);
    };
  }, []);

  if (navigated) {
    return (
      <p role="alert" className="card-stopped">
        This card tried to open another page and was stopped.
      </p>
    );
  }

  return (
    <iframe
      ref={frameRef}
      title="Card"
      src={cardFrameUrl}
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      style={{ display: "block", width: "100%", border: 0, height }}
    />
  );
}
