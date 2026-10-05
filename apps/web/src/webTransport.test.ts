import { CoreClient, CoreError } from "core-client";
import { expect, test } from "vitest";
import { createWebTransport } from "./webTransport";
import type { FromWorker, ToWorker, WorkerLike } from "./workerProtocol";

class FakeWorker implements WorkerLike {
  onmessage: ((event: { data: FromWorker }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  terminated = false;
  received: ToWorker[] = [];

  postMessage(message: ToWorker, transfer: Transferable[] = []) {
    // Like a real transfer: the worker gets the data and the sender's buffer is detached.
    this.received.push({
      ...message,
      bytes: message.bytes && Uint8Array.from(message.bytes),
    });
    structuredClone(null, { transfer });
  }
  terminate() {
    this.terminated = true;
  }
  send(data: FromWorker) {
    this.onmessage?.({ data });
  }
}

function setup() {
  const workers: FakeWorker[] = [];
  const transport = createWebTransport(() => {
    const worker = new FakeWorker();
    workers.push(worker);
    return worker;
  });
  const client = new CoreClient(transport);
  const latest = () => workers[workers.length - 1] as FakeWorker;
  return { workers, client, latest };
}

const reply = (id: number, output: unknown, bytes = new Uint8Array(0)): FromWorker => ({
  type: "reply",
  id,
  output: JSON.stringify(output),
  bytes,
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

test("a call waits for the worker, then resolves with the output", async () => {
  const { client, latest } = setup();
  const result = client.call("getCoreInfo", null);
  await tick();
  expect(latest().received).toEqual([]);
  latest().send({ type: "ready" });
  expect(latest().received[0]).toMatchObject({ method: "getCoreInfo", input: "null", op: 1 });
  latest().send(reply(1, { coreVersion: "0.0.0" }));
  expect(await result).toEqual({ coreVersion: "0.0.0" });
});

test("an ApiError from the core rejects as a CoreError", async () => {
  const { client, latest } = setup();
  latest().send({ type: "ready" });
  const result = client.call("exampleDivide", { dividend: 1, divisor: 0 });
  latest().send({
    type: "error",
    id: 1,
    error: JSON.stringify({ kind: "invalidInput", message: "Can't divide by zero." }),
  });
  await expect(result).rejects.toMatchObject({
    name: "CoreError",
    kind: "invalidInput",
    message: "Can't divide by zero.",
  });
});

test("calls run one at a time, in order", async () => {
  const { client, latest } = setup();
  latest().send({ type: "ready" });
  const first = client.call("getCoreInfo", null);
  const second = client.call("spikeListNotes", null);
  expect(latest().received.map((m) => m.id)).toEqual([1]);
  latest().send(reply(1, { coreVersion: "a" }));
  await first;
  expect(latest().received.map((m) => m.id)).toEqual([1, 2]);
  latest().send(reply(2, { notes: [] }));
  expect(await second).toEqual({ notes: [] });
});

test("cancelling a queued call rejects it without reaching the worker", async () => {
  const { client, latest } = setup();
  latest().send({ type: "ready" });
  const first = client.call("getCoreInfo", null);
  const controller = new AbortController();
  const second = client.call("spikeListNotes", null, { signal: controller.signal });
  controller.abort();
  await expect(second).rejects.toMatchObject({ kind: "cancelled" });
  latest().send(reply(1, { coreVersion: "a" }));
  await first;
  expect(latest().received.map((m) => m.method)).toEqual(["getCoreInfo"]);
});

test("cancelling the running call restarts the worker, and later calls succeed on the new one", async () => {
  const { client, workers, latest } = setup();
  latest().send({ type: "ready" });
  const controller = new AbortController();
  const slow = client.call("debugSlow", { steps: 5, stepMs: 100 }, { signal: controller.signal });
  const later = client.call("getCoreInfo", null);
  controller.abort();
  await expect(slow).rejects.toBeInstanceOf(CoreError);
  await expect(slow).rejects.toMatchObject({ kind: "cancelled" });
  expect(workers).toHaveLength(2);
  expect(workers[0]?.terminated).toBe(true);
  expect(workers[1]?.received).toEqual([]);
  workers[1]?.send({ type: "ready" });
  expect(workers[1]?.received[0]).toMatchObject({ method: "getCoreInfo" });
  workers[1]?.send(reply(2, { coreVersion: "b" }));
  expect(await later).toEqual({ coreVersion: "b" });
});

test("messages from a terminated worker are ignored", async () => {
  const { client, workers, latest } = setup();
  latest().send({ type: "ready" });
  const controller = new AbortController();
  const slow = client.call("debugSlow", { steps: 1, stepMs: 1 }, { signal: controller.signal });
  controller.abort();
  await slow.catch(() => {});
  workers[0]?.send({ type: "ready" });
  workers[0]?.send({ type: "notice", notice: "{}" });
  expect(workers[1]?.received).toEqual([]);
});

test("a trap rejects with a readable internal error and restarts the worker", async () => {
  const { client, workers, latest } = setup();
  latest().send({ type: "ready" });
  const crash = client.call("debugPanic", null);
  latest().send({ type: "trap", id: 1 });
  await expect(crash).rejects.toMatchObject({
    kind: "internal",
    message: "Something went wrong. Reload the page to continue.",
  });
  expect(workers).toHaveLength(2);
  expect(workers[0]?.terminated).toBe(true);
  const next = client.call("getCoreInfo", null);
  workers[1]?.send({ type: "ready" });
  workers[1]?.send(reply(2, { coreVersion: "c" }));
  expect(await next).toEqual({ coreVersion: "c" });
});

test("attachments are transferred, not copied", async () => {
  const { client, latest } = setup();
  latest().send({ type: "ready" });
  const bytes = Uint8Array.from([1, 2, 3]);
  const result = client.call("debugEchoBytes", null, { bytes });
  expect(bytes.buffer.byteLength).toBe(0);
  expect([...(latest().received[0]?.bytes ?? [])]).toEqual([1, 2, 3]);
  latest().send(reply(1, { length: 3 }, Uint8Array.from([1, 2, 3])));
  const { bytes: echoed } = await result;
  expect([...echoed]).toEqual([1, 2, 3]);
});

test("progress notices reach the call they belong to", async () => {
  const { client, latest } = setup();
  latest().send({ type: "ready" });
  const seenA: number[] = [];
  const a = client.call(
    "debugSlow",
    { steps: 2, stepMs: 1 },
    { onProgress: (p) => seenA.push(p.done) },
  );
  const progress = (op: number, done: number): FromWorker => ({
    type: "notice",
    notice: JSON.stringify({ type: "progress", op, progress: { done, total: 2, message: null } }),
  });
  latest().send(progress(1, 1));
  latest().send(progress(2, 9));
  latest().send(reply(1, { completed: 2 }));
  await a;
  expect(seenA).toEqual([1]);
});

test("when the collection cannot be opened every call gets the other-tab message", async () => {
  const { client, latest } = setup();
  const waiting = client.call("getCoreInfo", null);
  latest().send({ type: "openFailed" });
  const expected = {
    kind: "internal",
    message: "The collection is open in another tab. Close the other tab, then reload this one.",
  };
  await expect(waiting).rejects.toMatchObject(expected);
  await expect(client.call("getCoreInfo", null)).rejects.toMatchObject(expected);
});
