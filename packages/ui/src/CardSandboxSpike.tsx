import { CoreError } from "core-client";
import { useEffect, useRef, useState } from "react";
import { CardFrame } from "./card/CardFrame";
import { maliciousCard, mediaCard, navigatingCard, sampleCard } from "./cardSandboxSpikeCards";
import { useCore } from "./core";

/** Names the demo cards use for their media. */
const MEDIA = ["sample.png", "sample.wav"];

const MAX_HEIGHT_PX = 10_000;

type Demo = { label: string; html: string; media: string[] };

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? (sorted[middle] ?? 0)
    : ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2;
}

/**
 * Temporary panel for the card sandbox spike (step 0.6, ADR 0005). Shows a legitimate card, a
 * malicious one and two that navigate their frame, and the evidence that nothing got out. Removed
 * when real card rendering lands (Phase 2).
 */
export function CardSandboxSpike() {
  const core = useCore();
  const [demo, setDemo] = useState<Demo | null>(null);
  const [reachedCore, setReachedCore] = useState(false);
  const [reachedByChannel, setReachedByChannel] = useState(false);
  const [heights, setHeights] = useState<number[]>([]);
  const [timings, setTimings] = useState<number[]>([]);
  const [coreCheck, setCoreCheck] = useState<string | null>(null);
  const started = useRef(0);
  const firstHeight = useRef(true);
  const runs = useRef(0);
  // Proves the app page was not navigated or reloaded by a card.
  const pageStart = useRef(performance.timeOrigin);

  // A card that reaches the core would send this event. It must never arrive.
  useEffect(
    () =>
      core.onEvent((event) => {
        if (event.kind === "debug" && event.message === "FROM-CARD") setReachedCore(true);
      }),
    [core],
  );

  // A card can post on a BroadcastChannel too. An opaque origin should not reach this one.
  useEffect(() => {
    if (typeof BroadcastChannel === "undefined") return;
    const channel = new BroadcastChannel("fc-spike");
    channel.onmessage = () => setReachedByChannel(true);
    return () => channel.close();
  }, []);

  async function loadMedia(name: string): Promise<Blob> {
    const { output, bytes } = await core.call("spikeCardMedia", { name });
    // Copy into a plain ArrayBuffer-backed array: the reply may be a view on a larger buffer.
    return new Blob([new Uint8Array(bytes)], { type: output.contentType });
  }

  function show(label: string, html: string, media: string[]) {
    runs.current += 1;
    started.current = performance.now();
    firstHeight.current = true;
    setHeights([]);
    setCoreCheck(null);
    // The comment makes each press a new card, so the frame is created again.
    setDemo({ label, html: `${html}<!-- run ${runs.current} -->`, media });
  }

  function onHeight(px: number) {
    setHeights((all) => [...all, px]);
    if (firstHeight.current) {
      firstHeight.current = false;
      setTimings((all) => [...all, Math.round(performance.now() - started.current)]);
    }
  }

  // After a malicious card has had its turn, the core and the page must be as they were.
  useEffect(() => {
    if (demo?.label !== "Malicious card") return;
    const timer = setTimeout(async () => {
      const sameLoad = pageStart.current === performance.timeOrigin;
      try {
        const info = await core.call("getCoreInfo", null);
        setCoreCheck(
          `Core still answers (version ${info.coreVersion}). App page not reloaded: ${sameLoad ? "yes" : "NO"}.`,
        );
      } catch (error) {
        setCoreCheck(
          `Core check failed: ${error instanceof CoreError ? error.message : String(error)}`,
        );
      }
    }, 4500);
    return () => clearTimeout(timer);
  }, [core, demo]);

  const appUrl = window.location.href;
  const maxHeight = heights.length ? Math.max(...heights) : null;

  return (
    <section className="card-spike">
      <h2>Card sandbox spike</h2>
      <div className="row">
        <button type="button" onClick={() => show("Sample card", sampleCard(), MEDIA)}>
          Sample card
        </button>
        <button type="button" onClick={() => show("Media card", mediaCard(), MEDIA)}>
          Media card
        </button>
        <button
          type="button"
          onClick={() => show("Malicious card", maliciousCard(window.location.origin), [])}
        >
          Malicious card
        </button>
        <button
          type="button"
          onClick={() => show("Navigating card (app)", navigatingCard(appUrl), [])}
        >
          Navigating card (app)
        </button>
        <button
          type="button"
          onClick={() =>
            show("Navigating card (remote)", navigatingCard("https://example.com/"), [])
          }
        >
          Navigating card (remote)
        </button>
        <button type="button" onClick={() => setDemo(null)}>
          Clear
        </button>
      </div>
      {reachedCore && (
        <p role="alert" className="card-spike-alarm">
          A card reached the core
        </p>
      )}
      {reachedByChannel && (
        <p role="alert" className="card-spike-alarm">
          A card reached the app through a BroadcastChannel
        </p>
      )}
      {demo && (
        <>
          <p data-testid="card-spike-info">
            {demo.label}.{" "}
            {maxHeight === null
              ? "Waiting for the card."
              : `Height reported: ${heights[heights.length - 1]} px (largest ${maxHeight} px, limit ${MAX_HEIGHT_PX}).`}{" "}
            {timings.length > 0 &&
              `First height after ${timings[timings.length - 1]} ms (median of ${timings.length}: ${median(timings)} ms).`}
          </p>
          <div className="card-spike-box">
            <CardFrame
              html={demo.html}
              mediaNames={demo.media}
              loadMedia={loadMedia}
              onHeight={onHeight}
            />
          </div>
          {coreCheck && <p data-testid="card-spike-core-check">{coreCheck}</p>}
        </>
      )}
    </section>
  );
}
