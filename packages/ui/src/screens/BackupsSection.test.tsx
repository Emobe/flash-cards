import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { CoreClient, createFakeTransport } from "core-client";
import { afterEach, describe, expect, test } from "vitest";
import { CoreProvider } from "../core";
import { PlatformProvider } from "../platform";
import { BACKUP_HELP, BackupsSection } from "./BackupsSection";

afterEach(cleanup);

const entry = (name: string, createdMs: number | null, restorable = true) => ({
  name,
  createdMs,
  sizeBytes: 4096,
  restorable,
});

function renderSection(
  localBackups: boolean,
  handlers: Record<string, (input: never) => unknown> = {},
) {
  const calls: { method: string; input: unknown }[] = [];
  const wrap = (method: string, fallback: unknown) => (input: never) => {
    calls.push({ method, input });
    return handlers[method] ? handlers[method](input) : fallback;
  };
  const client = new CoreClient(
    createFakeTransport({
      getBackupSettings: wrap("getBackupSettings", {
        intervalHours: 24,
        keep: 5,
        lastError: null,
      }),
      listBackups: wrap("listBackups", {
        backups: [
          entry("backup-2.fcbackup", 1_791_363_600_000),
          entry("backup-1.fcbackup", null, false),
        ],
      }),
      backupNow: wrap("backupNow", entry("backup-3.fcbackup", 1)),
      setBackupSettings: wrap("setBackupSettings", { intervalHours: 72, keep: 3, lastError: null }),
      restoreListedBackup: wrap("restoreListedBackup", {
        registersWritten: 4,
        removed: 1,
        rejected: [],
      }),
    } as never),
  );
  render(
    <CoreProvider client={client}>
      <PlatformProvider
        platform={{ cardFrameUrl: "", localBackups, setSystemTheme: () => undefined }}
      >
        <BackupsSection />
      </PlatformProvider>
    </CoreProvider>,
  );
  return calls;
}

describe("Backups in Settings", () => {
  test("on the web there is no list, only export and files", () => {
    const calls = renderSection(false);
    expect(screen.queryByText("Back up now")).toBeNull();
    expect(screen.getByText("Export to a file")).toBeDefined();
    expect(screen.getByText("Restore from a file")).toBeDefined();
    expect(calls).toEqual([]);
  });

  test("lists the backups, with plain-language help, and a damaged one cannot be restored", async () => {
    renderSection(true);
    await screen.findByText("Back up now");
    expect(screen.getByText(BACKUP_HELP.interval)).toBeDefined();
    expect(screen.getByText(BACKUP_HELP.folder)).toBeDefined();
    const buttons = screen.getAllByRole("button", { name: "Restore" });
    expect(buttons).toHaveLength(2);
    expect((buttons[1] as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Unreadable file")).toBeDefined();
  });

  test("Back up now makes a backup and reloads the list", async () => {
    const calls = renderSection(true);
    fireEvent.click(await screen.findByText("Back up now"));
    await screen.findByText("Backup made.");
    expect(calls.filter((c) => c.method === "backupNow")).toHaveLength(1);
    expect(calls.filter((c) => c.method === "listBackups")).toHaveLength(2);
  });

  test("saves the interval and how many to keep, and refuses a keep that is not a number", async () => {
    const calls = renderSection(true);
    const interval = await screen.findByLabelText("Automatic backup");
    fireEvent.change(interval, { target: { value: "72" } });
    fireEvent.change(screen.getByLabelText("Backups to keep"), { target: { value: "0" } });
    fireEvent.click(screen.getByText("Save"));
    expect((await screen.findByRole("alert")).textContent).toMatch(/1 to 100/);
    expect(calls.some((c) => c.method === "setBackupSettings")).toBe(false);
    fireEvent.change(screen.getByLabelText("Backups to keep"), { target: { value: "3" } });
    fireEvent.click(screen.getByText("Save"));
    await screen.findByText("Saved.");
    expect(calls.find((c) => c.method === "setBackupSettings")?.input).toEqual({
      intervalHours: 72,
      keep: 3,
    });
  });

  test("shows why the last automatic backup failed", async () => {
    renderSection(true, {
      getBackupSettings: () => ({ intervalHours: 24, keep: 5, lastError: "The disk is full." }),
    });
    expect((await screen.findByRole("alert")).textContent).toMatch(/The disk is full/);
  });

  test("Restore asks first and Cancel changes nothing", async () => {
    const calls = renderSection(true);
    await screen.findByText("Back up now");
    fireEvent.click(screen.getAllByRole("button", { name: "Restore" })[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toMatch(/made first/);
    fireEvent.click(within(dialog).getByText("Cancel"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.some((c) => c.method === "restoreListedBackup")).toBe(false);
  });

  test("confirming restores the named backup and says what changed", async () => {
    const calls = renderSection(true);
    await screen.findByText("Back up now");
    fireEvent.click(screen.getAllByRole("button", { name: "Restore" })[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByText("Restore"));
    await screen.findByText(/Restored: 4 changes applied, 1 items moved to the trash/);
    expect(calls.find((c) => c.method === "restoreListedBackup")?.input).toEqual({
      name: "backup-2.fcbackup",
    });
  });
});
