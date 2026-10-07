import { CoreError } from "core-client";
import { type FormEvent, useState } from "react";
import { useCore } from "./core";

function message(error: unknown): string {
  return error instanceof CoreError ? error.message : "Something went wrong. Try again.";
}

/** Temporary demo of a call and an error path (step 0.3). Removed with the example methods. */
export function DivideForm() {
  const core = useCore();
  const [dividend, setDividend] = useState("10");
  const [divisor, setDivisor] = useState("4");
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    try {
      const out = await core.call("exampleDivide", {
        dividend: Number(dividend),
        divisor: Number(divisor),
      });
      setResult({ ok: true, text: `${dividend} ÷ ${divisor} = ${out.quotient}` });
    } catch (error) {
      setResult({ ok: false, text: message(error) });
    }
  }

  return (
    <form onSubmit={submit} className="divide">
      <label>
        Dividend
        <input
          type="number"
          inputMode="decimal"
          value={dividend}
          onChange={(e) => setDividend(e.target.value)}
        />
      </label>
      <label>
        Divisor
        <input
          type="number"
          inputMode="decimal"
          value={divisor}
          onChange={(e) => setDivisor(e.target.value)}
        />
      </label>
      <button type="submit">Divide</button>
      {result && (
        <p role={result.ok ? "status" : "alert"} className={result.ok ? "ok" : "error"}>
          {result.text}
        </p>
      )}
    </form>
  );
}
