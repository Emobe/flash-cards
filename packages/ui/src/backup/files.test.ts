import type { BackupInfo } from "core-client";
import { afterEach, expect, test, vi } from "vitest";
import { backupFileName, downloadBytes, readFileBytes } from "./files";

const info = (scope: BackupInfo["scope"]): BackupInfo => ({
  formatVersion: 1,
  appVersion: "0.0.0",
  storageVersion: 12,
  createdMs: Date.UTC(2026, 9, 7, 9, 0, 0),
  deviceId: "d",
  scope,
  history: true,
  registers: 1,
  rows: 0,
  mediaFiles: 0,
});

afterEach(() => vi.restoreAllMocks());

test("file names say what the file holds and when it was made", () => {
  expect(backupFileName(info({ kind: "collection" }))).toBe("flash-cards-2026-10-07.fcbackup");
  expect(backupFileName(info({ kind: "deck", id: "x", name: "Język polski: A1" }))).toBe(
    "flash-cards-język-polski-a1-2026-10-07.fcbackup",
  );
  // A name with no letters or digits falls back to the plain name.
  expect(backupFileName(info({ kind: "deck", id: "x", name: "???" }))).toBe(
    "flash-cards-2026-10-07.fcbackup",
  );
});

test("a download clicks a temporary link with the file name and cleans up", () => {
  vi.useFakeTimers();
  const created = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:fake");
  const revoked = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  let clicked: { href: string; download: string } | undefined;
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
    this: HTMLAnchorElement,
  ) {
    clicked = { href: this.href, download: this.download };
  });

  downloadBytes(new Uint8Array([80, 75]), "a.fcbackup");

  expect(clicked).toEqual({ href: "blob:fake", download: "a.fcbackup" });
  expect(created.mock.calls[0]?.[0]).toBeInstanceOf(Blob);
  expect(document.querySelector("a[download]")).toBeNull();
  expect(revoked).not.toHaveBeenCalled();
  vi.advanceTimersByTime(10_000);
  expect(revoked).toHaveBeenCalledWith("blob:fake");
  vi.useRealTimers();
});

test("a chosen file is read whole", async () => {
  const bytes = await readFileBytes(new Blob([new Uint8Array([1, 2, 3])]));
  expect(Array.from(bytes)).toEqual([1, 2, 3]);
});
