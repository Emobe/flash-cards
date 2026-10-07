import type { BackupInfo } from "core-client";

/** `flash-cards-2026-10-07.fcbackup`, or `flash-cards-polish-2026-10-07.fcbackup` for a deck. */
export function backupFileName(info: BackupInfo): string {
  const day = new Date(info.createdMs).toISOString().slice(0, 10);
  const deck =
    info.scope.kind === "deck"
      ? `-${info.scope.name
          .toLowerCase()
          .replace(/[^\p{L}\p{N}]+/gu, "-")
          .replace(/^-|-$/g, "")}`
      : "";
  return `flash-cards${deck === "-" ? "" : deck}-${day}.fcbackup`;
}

/** Hands `bytes` to the browser as a download. The browser decides where it goes. */
export function downloadBytes(bytes: Uint8Array, fileName: string): void {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "application/zip" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.append(link);
  link.click();
  link.remove();
  // The download has started by the next task. Revoking sooner can cancel it in some browsers.
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/** The whole file as bytes. */
export async function readFileBytes(file: Blob): Promise<Uint8Array> {
  return new Uint8Array(await file.arrayBuffer());
}
