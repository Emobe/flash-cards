import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { CoreClient } from "core-client";
import { afterEach, expect, test } from "vitest";
import { createTauriTransport, decodeFrame } from "./tauriTransport";

afterEach(clearMocks);

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
  mockIPC((cmd, payload) => {
    seen = { cmd, payload };
    return frameOf('{"quotient":3.5}');
  });
  const client = new CoreClient(createTauriTransport());
  const out = await client.call("exampleDivide", { dividend: 7, divisor: 2 });
  expect(out).toEqual({ quotient: 3.5 });
  expect(seen).toEqual({
    cmd: "call",
    payload: { method: "exampleDivide", input: { dividend: 7, divisor: 2 } },
  });
});

test("a core error rejects as a CoreError with its message", async () => {
  mockIPC(() => {
    throw { kind: "invalidInput", message: "Can't divide by zero. Enter a divisor other than 0." };
  });
  const client = new CoreClient(createTauriTransport());
  await expect(client.call("exampleDivide", { dividend: 1, divisor: 0 })).rejects.toMatchObject({
    kind: "invalidInput",
    message: "Can't divide by zero. Enter a divisor other than 0.",
  });
});

test("a non-core failure becomes an internal error without leaking its text", async () => {
  mockIPC(() => {
    throw "command call not allowed by ACL";
  });
  const client = new CoreClient(createTauriTransport());
  const error = await client.call("getCoreInfo", null).catch((e: unknown) => e);
  expect(error).toMatchObject({ kind: "internal" });
  expect((error as Error).message).not.toContain("ACL");
});
