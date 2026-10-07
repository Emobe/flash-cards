import {
  type BackupEntry,
  type BackupSettingsOutput,
  CoreError,
  type RestoreOutput,
} from "core-client";
import { type FormEvent, useCallback, useEffect, useId, useRef, useState } from "react";
import { backupFileName, downloadBytes, readFileBytes } from "../backup/files";
import { Dialog } from "../components/Dialog";
import { useCore } from "../core";
import { usePlatform } from "../platform";

const INTERVALS: { hours: number; label: string }[] = [
  { hours: 0, label: "Never" },
  { hours: 24, label: "Once a day" },
  { hours: 72, label: "Every 3 days" },
  { hours: 168, label: "Once a week" },
];

export const BACKUP_HELP = {
  interval:
    "The app makes a backup when it starts, if the last one is older than this. It never makes one while you are using it.",
  keep: "How many backups to keep. When there are more, the oldest are deleted.",
  folder:
    "Backups are kept inside the app's own storage on this device. Uninstalling the app deletes them, and a lost phone takes them with it. Use Export to save a copy somewhere safe.",
  export:
    "Saves your cards and settings to a file you can keep anywhere. Without the review history the file is smaller, but your learning progress is not in it.",
  restore:
    "Makes the collection match the file. Cards and changes made after the backup go to the trash, so you can still bring them back.",
  import: "Adds what the file has to your collection and deletes nothing.",
} as const;

function message(error: unknown): string {
  return error instanceof CoreError ? error.message : "Something went wrong. Try again.";
}

function describe(what: string, out: RestoreOutput): string {
  const parts = [`${out.registersWritten} changes applied`];
  if (out.removed > 0) parts.push(`${out.removed} items moved to the trash`);
  if (out.rejected.length > 0) parts.push(`${out.rejected.length} could not be used`);
  return `${what}: ${parts.join(", ")}.`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function when(ms: number | null): string {
  return ms === null ? "Unreadable file" : new Date(ms).toLocaleString();
}

type Pending =
  | { kind: "listed"; entry: BackupEntry }
  | { kind: "file"; bytes: Uint8Array; createdMs: number }
  | null;

/**
 * Backups in Settings (step 2.6). On a device with a backups folder (the native apps): the
 * automatic backup settings, Back up now, and the list with Restore. On every platform: export to
 * a file, restore from a file and import a file.
 */
export function BackupsSection() {
  const core = useCore();
  const { localBackups } = usePlatform();
  const [settings, setSettings] = useState<BackupSettingsOutput | null>(null);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending>(null);
  const [history, setHistory] = useState(true);
  const fileInput = useRef<HTMLInputElement>(null);
  const [fileAction, setFileAction] = useState<"restore" | "import">("restore");
  const fileId = useId();

  const load = useCallback(async () => {
    try {
      if (localBackups) {
        const [s, l] = await Promise.all([
          core.call("getBackupSettings", null),
          core.call("listBackups", null),
        ]);
        setSettings(s);
        setBackups(l.backups);
      }
    } catch (e) {
      setProblem(message(e));
    }
  }, [core, localBackups]);

  useEffect(() => {
    void load();
  }, [load]);

  async function run(work: () => Promise<string | null>) {
    setProblem(null);
    setStatus(null);
    setBusy(true);
    try {
      setStatus(await work());
    } catch (e) {
      setProblem(message(e));
    }
    setBusy(false);
    void load();
  }

  const backUpNow = () =>
    run(async () => {
      await core.call("backupNow", null);
      return "Backup made.";
    });

  const exportAll = () =>
    run(async () => {
      const { output, bytes } = await core.call("exportBackup", { deck: null, history });
      downloadBytes(bytes, backupFileName(output.info));
      return "Export started. The file is named like flash-cards-2026-10-07.fcbackup.";
    });

  function chooseFile(action: "restore" | "import") {
    setFileAction(action);
    if (fileInput.current) {
      fileInput.current.value = "";
      fileInput.current.click();
    }
  }

  async function fileChosen() {
    const chosen = fileInput.current?.files?.[0];
    if (!chosen) return;
    await run(async () => {
      const bytes = await readFileBytes(chosen);
      // The transport hands the buffer to the worker, so send a copy for reading it.
      const info = await core.call("readBackupInfo", null, { bytes: bytes.slice() });
      if (fileAction === "import") {
        return describe("Imported", await core.call("importBackup", null, { bytes }));
      }
      if (info.scope.kind !== "collection") {
        throw new CoreError(
          "invalidInput",
          "This file holds one deck, so it can be imported but not restored.",
        );
      }
      setPending({ kind: "file", bytes, createdMs: info.createdMs });
      return null;
    });
  }

  async function confirmRestore() {
    const target = pending;
    setPending(null);
    if (!target) return;
    await run(async () => {
      const out =
        target.kind === "listed"
          ? await core.call("restoreListedBackup", { name: target.entry.name })
          : await core.call("restoreBackup", null, { bytes: target.bytes });
      return describe("Restored", out);
    });
  }

  return (
    <section className="options-section" aria-labelledby={`${fileId}-h`}>
      <h2 id={`${fileId}-h`}>Backups</h2>
      {settings?.lastError && (
        <p role="alert" className="dialog-error">
          The last automatic backup failed: {settings.lastError}
        </p>
      )}
      {localBackups && settings && (
        <>
          <SettingsForm settings={settings} onSaved={load} />
          <div className="options-field">
            <div className="options-buttons">
              <button type="button" className="button" disabled={busy} onClick={backUpNow}>
                Back up now
              </button>
            </div>
            <p className="options-help">{BACKUP_HELP.folder}</p>
            {backups.length === 0 ? (
              <p className="options-help">No backups yet.</p>
            ) : (
              <ul className="backup-list">
                {backups.map((entry) => (
                  <li key={entry.name}>
                    <span>
                      {when(entry.createdMs)}
                      <span className="options-help"> · {formatSize(entry.sizeBytes)}</span>
                    </span>
                    <button
                      type="button"
                      className="button"
                      disabled={busy || !entry.restorable}
                      onClick={() => setPending({ kind: "listed", entry })}
                    >
                      Restore
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </>
      )}
      <div className="options-field">
        <label>
          <input type="checkbox" checked={history} onChange={(e) => setHistory(e.target.checked)} />{" "}
          Include review history
        </label>
        <div className="options-buttons">
          <button type="button" className="button" disabled={busy} onClick={exportAll}>
            Export to a file
          </button>
        </div>
        <p className="options-help">{BACKUP_HELP.export}</p>
      </div>
      <div className="options-field">
        <div className="options-buttons">
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => chooseFile("restore")}
          >
            Restore from a file
          </button>
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => chooseFile("import")}
          >
            Import a file
          </button>
        </div>
        <p className="options-help">
          {BACKUP_HELP.restore} {BACKUP_HELP.import}
        </p>
        <input
          ref={fileInput}
          type="file"
          accept=".fcbackup,.zip"
          hidden
          aria-label="Backup file"
          onChange={fileChosen}
        />
      </div>
      {problem && (
        <p role="alert" className="dialog-error">
          {problem}
        </p>
      )}
      {status && <p role="status">{status}</p>}
      {pending && (
        <Dialog title="Restore backup" onClose={() => setPending(null)}>
          <div className="dialog-form">
            <p>
              Restore the backup made{" "}
              <strong>
                {when(pending.kind === "listed" ? pending.entry.createdMs : pending.createdMs)}
              </strong>
              ? {BACKUP_HELP.restore}
              {pending.kind === "listed" &&
                " A backup of the collection as it is now is made first, so you can undo this by restoring that one."}
            </p>
            <div className="dialog-actions">
              <button type="button" className="button" onClick={() => setPending(null)}>
                Cancel
              </button>
              <button type="button" className="button button-danger" onClick={confirmRestore}>
                Restore
              </button>
            </div>
          </div>
        </Dialog>
      )}
    </section>
  );
}

function SettingsForm({
  settings,
  onSaved,
}: {
  settings: BackupSettingsOutput;
  onSaved: () => Promise<void>;
}) {
  const core = useCore();
  const base = useId();
  const [hours, setHours] = useState(settings.intervalHours);
  const [keep, setKeep] = useState(String(settings.keep));
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const intervals = INTERVALS.some((i) => i.hours === settings.intervalHours)
    ? INTERVALS
    : [
        ...INTERVALS,
        { hours: settings.intervalHours, label: `Every ${settings.intervalHours} hours` },
      ];

  async function save(event: FormEvent) {
    event.preventDefault();
    setSaved(false);
    if (!/^\d+$/.test(keep) || Number(keep) < 1 || Number(keep) > 100) {
      setError("Keep a whole number of backups from 1 to 100.");
      return;
    }
    try {
      await core.call("setBackupSettings", { intervalHours: hours, keep: Number(keep) });
      setError(null);
      setSaved(true);
      await onSaved();
    } catch (e) {
      setError(message(e));
    }
  }

  return (
    <form onSubmit={save} noValidate>
      <div className="options-field">
        <label htmlFor={`${base}-i`}>Automatic backup</label>
        <select
          id={`${base}-i`}
          value={hours}
          aria-describedby={`${base}-ih`}
          onChange={(e) => {
            setHours(Number(e.target.value));
            setSaved(false);
          }}
        >
          {intervals.map((i) => (
            <option key={i.hours} value={i.hours}>
              {i.label}
            </option>
          ))}
        </select>
        <p id={`${base}-ih`} className="options-help">
          {BACKUP_HELP.interval}
        </p>
      </div>
      <div className="options-field">
        <label htmlFor={`${base}-k`}>Backups to keep</label>
        <input
          id={`${base}-k`}
          inputMode="numeric"
          value={keep}
          aria-describedby={`${base}-kh`}
          onChange={(e) => {
            setKeep(e.target.value);
            setSaved(false);
          }}
        />
        <p id={`${base}-kh`} className="options-help">
          {BACKUP_HELP.keep}
        </p>
      </div>
      {error && (
        <p role="alert" className="dialog-error">
          {error}
        </p>
      )}
      <div className="options-save">
        <button type="submit" className="button button-primary">
          Save
        </button>
        {saved && <span role="status">Saved.</span>}
      </div>
    </form>
  );
}
