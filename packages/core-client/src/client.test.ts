import { expect, test } from "vitest";
import { CoreClient, CoreError, createFakeTransport } from "./index";

test("call returns the typed output", async () => {
  const client = new CoreClient(
    createFakeTransport({ getCoreInfo: () => ({ coreVersion: "1.2.3" }) }),
  );
  const info = await client.call("getCoreInfo", null);
  expect(info.coreVersion).toBe("1.2.3");
});

test("an ApiError-shaped rejection becomes a CoreError", async () => {
  const client = new CoreClient({
    call: () => Promise.reject({ kind: "invalidInput", message: "Nope." }),
  });
  const error = await client.call("getCoreInfo", null).catch((e: unknown) => e);
  expect(error).toBeInstanceOf(CoreError);
  expect(error).toMatchObject({ kind: "invalidInput", message: "Nope." });
});

test("an unexpected rejection becomes an internal CoreError with a readable message", async () => {
  const client = new CoreClient({ call: () => Promise.reject(new Error("boom")) });
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
