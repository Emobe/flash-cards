import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { CoreClient, type Notice } from "core-client";
import { afterEach, expect, test, vi } from "vitest";
import { createTauriTransport, decodeFrame, encodeBase64 } from "./tauriTransport";

afterEach(clearMocks);

const TOKEN = "test-token";

/** `mockIPC` that also answers `handshake`, like the Rust side does once per page load. */
function mockTauri(handler: (cmd: string, payload: unknown) => unknown) {
  mockIPC((cmd, payload) => (cmd === "handshake" ? TOKEN : handler(cmd, payload)));
}

function frameOf(json: string, attachment: number[] = []): number[] {
  const body = new TextEncoder().encode(json);
  const header = [body.length & 255, (body.length >> 8) & 255, 0, 0];
  return [...header, ...body, ...attachment];
}

test("decodes a frame from an ArrayBuffer", () => {
  const bytes = Uint8Array.from(frameOf('{"a":1}', [7, 8]));
  const { output, bytes: attachment } = decodeFrame(bytes.buffer);
  expect(output).toEqual({ a: 1 });
  expect([...attachment]).toEqual([7, 8]);
});

test("decodes a frame from a number array", () => {
  const { output, bytes } = decodeFrame(frameOf('{"b":"é"}'));
  expect(output).toEqual({ b: "é" });
  expect(bytes.length).toBe(0);
});

test("call sends the method and input to the call command and returns the output", async () => {
  let seen: unknown;
  mockTauri((cmd, payload) => {
    seen = { cmd, payload };
    return frameOf('{"quotient":3.5}');
  });
  const client = new CoreClient(createTauriTransport());
  const out = await client.call("exampleDivide", { dividend: 7, divisor: 2 });
  expect(out).toEqual({ quotient: 3.5 });
  expect(seen).toEqual({
    cmd: "call",
    payload: { token: TOKEN, method: "exampleDivide", input: { dividend: 7, divisor: 2 }, op: 1 },
  });
});

test("a core error rejects as a CoreError with its message", async () => {
  mockTauri(() => {
    throw { kind: "invalidInput", message: "Can't divide by zero. Enter a divisor other than 0." };
  });
  const client = new CoreClient(createTauriTransport());
  await expect(client.call("exampleDivide", { dividend: 1, divisor: 0 })).rejects.toMatchObject({
    kind: "invalidInput",
    message: "Can't divide by zero. Enter a divisor other than 0.",
  });
});

test("a non-core failure becomes an internal error without leaking its text", async () => {
  mockTauri(() => {
    throw "command call not allowed by ACL";
  });
  const client = new CoreClient(createTauriTransport());
  const error = await client.call("getCoreInfo", null).catch((e: unknown) => e);
  expect(error).toMatchObject({ kind: "internal" });
  expect((error as Error).message).not.toContain("ACL");
});

type Internals = { runCallback(id: number, data: unknown): void };
const internals = () =>
  (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;

/** Mocks the commands, keeping the notice channel so a test can push notices into it. */
function mockCore(onCall: (payload: Record<string, unknown>) => number[] = () => frameOf("null")) {
  const state = {
    channelId: undefined as number | undefined,
    commands: [] as string[],
    calls: [] as Record<string, unknown>[],
    nextIndex: 0,
    push(notice: Notice) {
      if (state.channelId === undefined) throw new Error("not subscribed");
      internals().runCallback(state.channelId, { index: state.nextIndex++, message: notice });
    },
  };
  mockTauri((cmd, payload) => {
    state.commands.push(cmd);
    const args = payload as Record<string, unknown>;
    if (cmd === "subscribe") state.channelId = (args.onNotice as { id: number }).id;
    if (cmd === "call") {
      state.calls.push(args);
      return onCall(args);
    }
    return undefined;
  });
  return state;
}

test("base64 encoding matches btoa, with and without the native method", () => {
  const bytes = Uint8Array.from({ length: 70_000 }, (_, i) => i % 256);
  expect(encodeBase64(new Uint8Array())).toBe("");
  const viaFallback = (() => {
    const copy = new Uint8Array(bytes);
    Object.defineProperty(copy, "toBase64", { value: undefined });
    return encodeBase64(copy);
  })();
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  expect(viaFallback).toBe(btoa(binary));
  expect(encodeBase64(bytes)).toBe(btoa(binary));
});

test("an attachment is sent as base64 and the reply attachment comes back as bytes", async () => {
  const state = mockCore(() => frameOf('{"length":3}', [1, 2, 3]));
  const client = new CoreClient(createTauriTransport());
  const reply = await client.call("debugEchoBytes", null, { bytes: Uint8Array.of(1, 2, 3) });
  expect(state.calls[0]?.attachment).toBe("AQID");
  expect(reply.output).toEqual({ length: 3 });
  expect([...reply.bytes]).toEqual([1, 2, 3]);
});

test("a call without an attachment sends none", async () => {
  const state = mockCore();
  await new CoreClient(createTauriTransport()).call("getCoreInfo", null);
  expect(state.calls[0]?.attachment).toBeUndefined();
});

test("subscribing registers a channel before the next call, and notices reach onProgress and onEvent", async () => {
  const state = mockCore((args) => {
    // The core would send progress while the call runs.
    state.push({
      type: "progress",
      op: args.op as number,
      progress: { done: 1, total: 2, message: null },
    });
    state.push({ type: "event", event: { kind: "debug", message: "hello" } });
    state.push({
      type: "progress",
      op: args.op as number,
      progress: { done: 2, total: 2, message: null },
    });
    return frameOf('{"completed":2}');
  });
  const client = new CoreClient(createTauriTransport());
  const events: string[] = [];
  client.onEvent((e) => events.push(e.kind === "debug" ? e.message : e.kind));
  const seen: number[] = [];
  await client.call("debugSlow", { steps: 2, stepMs: 0 }, { onProgress: (p) => seen.push(p.done) });
  expect(state.commands.slice(0, 2)).toEqual(["subscribe", "call"]);
  expect(seen).toEqual([1, 2]);
  expect(events).toEqual(["hello"]);
});

test("aborting sends cancel with the operation id", async () => {
  const cancels: unknown[] = [];
  mockTauri((cmd, payload) => {
    if (cmd === "cancel") cancels.push(payload);
    if (cmd === "call") throw { kind: "cancelled", message: "The operation was cancelled." };
    return undefined;
  });
  const client = new CoreClient(createTauriTransport());
  const controller = new AbortController();
  const pending = client.call("debugSlow", { steps: 1, stepMs: 0 }, { signal: controller.signal });
  controller.abort();
  await expect(pending).rejects.toMatchObject({ kind: "cancelled" });
  await vi.waitFor(() => expect(cancels).toEqual([{ token: TOKEN, op: 1 }]));
});

test("the token from handshake goes to call, subscribe and cancel", async () => {
  const seen: Record<string, unknown>[] = [];
  mockTauri((cmd, payload) => {
    seen.push({ cmd, ...(payload as object) });
    if (cmd === "call") throw { kind: "cancelled", message: "The operation was cancelled." };
    return undefined;
  });
  const client = new CoreClient(createTauriTransport());
  client.onEvent(() => {});
  const controller = new AbortController();
  const pending = client.call("getCoreInfo", null, { signal: controller.signal });
  controller.abort();
  await pending.catch(() => {});
  await vi.waitFor(() => expect(seen.map((s) => s.cmd)).toContain("cancel"));
  for (const cmd of ["subscribe", "call", "cancel"]) {
    expect(seen.find((s) => s.cmd === cmd)?.token, cmd).toBe(TOKEN);
  }
});

test("the handshake is requested once, when the transport is created", async () => {
  const handshakes: number[] = [];
  mockIPC((cmd) => {
    if (cmd === "handshake") handshakes.push(1);
    return cmd === "handshake" ? TOKEN : frameOf("null");
  });
  const client = new CoreClient(createTauriTransport());
  expect(handshakes.length).toBe(1);
  await client.call("getCoreInfo", null);
  await client.call("getCoreInfo", null);
  expect(handshakes.length).toBe(1);
});

test("a failed handshake rejects calls with its message and never reaches the core", async () => {
  const commands: string[] = [];
  mockIPC((cmd) => {
    commands.push(cmd);
    throw { kind: "internal", message: "The app could not connect to its core. Restart the app." };
  });
  const client = new CoreClient(createTauriTransport());
  await expect(client.call("getCoreInfo", null)).rejects.toMatchObject({
    kind: "internal",
    message: "The app could not connect to its core. Restart the app.",
  });
  expect(commands).toEqual(["handshake"]);
});
