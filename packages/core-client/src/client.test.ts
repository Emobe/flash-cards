import { expect, test } from "vitest";
import { CoreClient, CoreError, createFakeTransport, type Progress, type Transport } from "./index";

/** A transport whose `call` is `call`, for tests of error mapping. */
function transportWith(call: Transport["call"]): Transport {
  return { call, cancel: () => {}, subscribe: () => () => {} };
}

test("call returns the typed output", async () => {
  const client = new CoreClient(
    createFakeTransport({ getCoreInfo: () => ({ coreVersion: "1.2.3" }) }),
  );
  const info = await client.call("getCoreInfo", null);
  expect(info.coreVersion).toBe("1.2.3");
});

test("an ApiError-shaped rejection becomes a CoreError", async () => {
  const client = new CoreClient(
    transportWith(() => Promise.reject({ kind: "invalidInput", message: "Nope." })),
  );
  const error = await client.call("getCoreInfo", null).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(CoreError);
  expect(error).toMatchObject({ kind: "invalidInput", message: "Nope." });
});

test("an unexpected rejection becomes an internal CoreError with a readable message", async () => {
  const client = new CoreClient(transportWith(() => Promise.reject(new Error("boom"))));
  const error = await client.call("getCoreInfo", null).catch((e: unknown) => e);
  expect(error).toMatchObject({ kind: "internal" });
  expect((error as CoreError).message).not.toContain("boom");
});

test("a missing fake handler is an unknownMethod error", async () => {
  const client = new CoreClient(createFakeTransport({}));
  await expect(client.call("getCoreInfo", null)).rejects.toMatchObject({ kind: "unknownMethod" });
});

test("calls are typed from the generated Methods", () => {
  const client = new CoreClient(createFakeTransport({}));
  // @ts-expect-error divisor must be a number
  void client.call("exampleDivide", { dividend: 1, divisor: "2" }).catch(() => {});
  // @ts-expect-error unknown method name
  void client.call("noSuchMethod", null).catch(() => {});
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

test("progress reaches the caller that asked for it, in order, and no other", async () => {
  const transport = createFakeTransport({
    debugSlow: async ({ steps }, ctx) => {
      for (let done = 1; done <= steps; done++) {
        ctx.progress({ done, total: steps, message: null });
        await tick();
      }
      return { completed: steps };
    },
  });
  const client = new CoreClient(transport);
  const mine: number[] = [];
  const other: Progress[] = [];
  const [out] = await Promise.all([
    client.call("debugSlow", { steps: 3, stepMs: 0 }, { onProgress: (p) => mine.push(p.done) }),
    client.call("debugSlow", { steps: 2, stepMs: 0 }, { onProgress: (p) => other.push(p) }),
  ]);
  expect(out.completed).toBe(3);
  expect(mine).toEqual([1, 2, 3]);
  expect(other.map((p) => p.done)).toEqual([1, 2]);
});

test("aborting the signal cancels the operation and rejects with kind cancelled", async () => {
  const transport = createFakeTransport({
    debugSlow: async (_, ctx) => {
      for (let i = 0; i < 100; i++) {
        ctx.checkpoint();
        await tick();
      }
      return { completed: 100 };
    },
  });
  const client = new CoreClient(transport);
  const controller = new AbortController();
  const pending = client.call(
    "debugSlow",
    { steps: 100, stepMs: 0 },
    { signal: controller.signal },
  );
  await tick();
  controller.abort();
  await expect(pending).rejects.toMatchObject({ kind: "cancelled" });
  expect(transport.cancelled).toHaveLength(1);
});

test("a signal that is already aborted never reaches the transport", async () => {
  const transport = createFakeTransport({ debugSlow: () => ({ completed: 0 }) });
  const client = new CoreClient(transport);
  const controller = new AbortController();
  controller.abort();
  await expect(
    client.call("debugSlow", { steps: 1, stepMs: 0 }, { signal: controller.signal }),
  ).rejects.toMatchObject({ kind: "cancelled" });
  expect(transport.cancelled).toHaveLength(0);
});

test("a cancel that reaches the core before the call starts still stops it", async () => {
  const transport = createFakeTransport({
    debugSlow: (_, ctx) => {
      ctx.checkpoint();
      return { completed: 1 };
    },
  });
  transport.cancel(1);
  const client = new CoreClient(transport);
  await expect(client.call("debugSlow", { steps: 1, stepMs: 0 })).rejects.toMatchObject({
    kind: "cancelled",
  });
});

test("attachments round trip and the reply has bytes", async () => {
  const client = new CoreClient(
    createFakeTransport({
      debugEchoBytes: (_, ctx) => {
        const bytes = ctx.bytes ?? new Uint8Array();
        ctx.setBytes(bytes);
        return { length: bytes.length };
      },
    }),
  );
  const { output, bytes } = await client.call("debugEchoBytes", null, {
    bytes: Uint8Array.of(1, 2, 3),
  });
  expect(output.length).toBe(3);
  expect([...bytes]).toEqual([1, 2, 3]);
});

test("an empty attachment still resolves to { output, bytes }", async () => {
  const client = new CoreClient(createFakeTransport({ debugEchoBytes: () => ({ length: 0 }) }));
  const reply = await client.call("debugEchoBytes", null, { bytes: new Uint8Array() });
  expect(reply.bytes.length).toBe(0);
});

test("events reach listeners until they unsubscribe", async () => {
  const transport = createFakeTransport({});
  const client = new CoreClient(transport);
  const seen: string[] = [];
  const stop = client.onEvent((e) => seen.push(e.kind === "debug" ? e.message : e.kind));
  transport.emit({ kind: "debug", message: "one" });
  stop();
  transport.emit({ kind: "debug", message: "two" });
  expect(seen).toEqual(["one"]);
});

test("attachment options are typed from the method", () => {
  const client = new CoreClient(createFakeTransport({}));
  // @ts-expect-error debugEchoBytes needs an attachment
  void client.call("debugEchoBytes", null).catch(() => {});
  // @ts-expect-error getCoreInfo does not take an attachment
  void client.call("getCoreInfo", null, { bytes: new Uint8Array() }).catch(() => {});
});
