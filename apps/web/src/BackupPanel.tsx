import { CoreError, type RestoreOutput } from "core-client";
import { useRef, useState } from "react";
import { useCore } from "ui";
import { backupFileName, downloadBytes, readFileBytes } from "./backupFiles";

/**
 * Temporary dev panel (step 1.13b): export, restore and import in the browser until the settings
 * screen (step 2.6) has the real ones. Uses the bytes methods, the only ones the web has.
 */
export function BackupPanel() {
  const core = useCore();
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const file = useRef<HTMLInputElement>(null);

  async function run(work: () => Promise<string>) {
    setError("");
    setStatus("");
    setBusy(true);
    try {
      setStatus(await work());
    } catch (e) {
      setError(e instanceof CoreError ? `${e.kind}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  function exportAll(history: boolean) {
    return run(async () => {
      const { output, bytes } = await core.call("exportBackup", { deck: null, history });
      downloadBytes(bytes, backupFileName(output.info));
      return `Exported ${output.info.registers} values and ${output.info.mediaFiles} media files (${bytes.length} bytes).`;
    });
  }

  async function chosenFile(): Promise<Uint8Array | undefined> {
    const chosen = file.current?.files?.[0];
    if (!chosen) throw new Error("Choose a backup file first.");
    return readFileBytes(chosen);
  }

  function summary(what: string, out: RestoreOutput): string {
    return `${what}: ${out.registersWritten} values written, ${out.registersUnchanged} already the same, ${out.rowsAdded} reviews added, ${out.removed} moved to the trash, ${out.rejected.length} rejected.`;
  }

  function restore() {
    return run(async () => {
      const bytes = await chosenFile();
      if (!bytes) return "";
      // The transport hands the buffer to the worker, which leaves nothing to send again.
      const info = await core.call("readBackupInfo", null, { bytes: bytes.slice() });
      const when = new Date(info.createdMs).toLocaleString();
      if (
        !window.confirm(
          `Restore the backup made ${when}? Anything added since then goes to the trash.`,
        )
      ) {
        return "Nothing was changed.";
      }
      return summary("Restored", await core.call("restoreBackup", null, { bytes }));
    });
  }

  function importFile() {
    return run(async () => {
      const bytes = await chosenFile();
      if (!bytes) return "";
      return summary("Imported", await core.call("importBackup", null, { bytes }));
    });
  }

  return (
    <section className="spike">
      <h2>Backup (temporary)</h2>
      <div className="row">
        <button type="button" disabled={busy} onClick={() => exportAll(true)}>
          Export backup
        </button>
        <button type="button" disabled={busy} onClick={() => exportAll(false)}>
          Export without history
        </button>
      </div>
      <div className="row">
        <input ref={file} type="file" accept=".fcbackup,.zip" aria-label="Backup file" />
        <button type="button" disabled={busy} onClick={restore}>
          Restore
        </button>
        <button type="button" disabled={busy} onClick={importFile}>
          Import
        </button>
      </div>
      {status && <p role="status">{status}</p>}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </section>
  );
}
