import { CoreError } from "core-client";
import { useState } from "react";
import { useCore } from "./core";

/** Temporary check of FSRS on every target (step 0.5). Removed with the spike methods. */
export function SchedulingSpike() {
  const core = useCore();
  const [intervals, setIntervals] = useState<string | null>(null);
  const [optimised, setOptimised] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const fail = (e: unknown) =>
    setError(e instanceof CoreError ? e.message : `Something went wrong: ${String(e)}`);

  async function schedule() {
    setError(null);
    try {
      const out = await core.call("spikeSchedule", {
        ratings: ["good", "good", "good", "good", "good"],
        desiredRetention: 0.9,
      });
      setIntervals(out.reviews.map((r) => r.intervalDays).join(", "));
    } catch (e) {
      fail(e);
    }
  }

  async function optimise() {
    setError(null);
    setBusy(true);
    // Let the "Optimising..." state paint before a native or wasm call blocks the thread.
    await new Promise((resolve) => setTimeout(resolve, 50));
    const started = performance.now();
    try {
      const out = await core.call("spikeOptimise", { cards: 200 });
      const ms = Math.round(performance.now() - started);
      setOptimised(
        `${out.parameters.length} parameters in ${ms} ms, w0..w2 = ${out.parameters
          .slice(0, 3)
          .map((p) => p.toFixed(3))
          .join(", ")}`,
      );
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2>Scheduling spike</h2>
      <div className="row">
        <button type="button" onClick={schedule}>
          Schedule 5 Good reviews
        </button>
        <button type="button" onClick={optimise} disabled={busy}>
          Optimise on 200 cards
        </button>
      </div>
      {intervals && <p data-testid="intervals">Intervals in days: {intervals}</p>}
      {busy && <p>Optimising...</p>}
      {optimised && <p data-testid="optimised">{optimised}</p>}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </section>
  );
}
