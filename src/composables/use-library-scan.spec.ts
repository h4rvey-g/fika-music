import { beforeEach, describe, expect, it, vi } from "vitest";
import { useLibraryScan } from "./use-library-scan";
import { createScanStatus } from "../test/fixtures";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

const status = createScanStatus({
  folderPath: "/music",
  discoveredFiles: 2,
  scannedFiles: 2,
  indexedTracks: 2,
  startedAt: 1,
  finishedAt: 2,
});

describe("library scan", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    tauri.invoke.mockResolvedValue(status);
    tauri.listen.mockResolvedValue(vi.fn());
  });

  it("loads status and subscribes to scan progress", async () => {
    const scan = useLibraryScan(vi.fn());

    await scan.initialize();

    expect(scan.selectedFolder.value).toBe("/music");
    expect(tauri.listen).toHaveBeenCalledWith("library:scan-progress", expect.any(Function));
  });

  it("starts a rescan when a download lands inside the library", async () => {
    const scan = useLibraryScan(vi.fn());
    await scan.initialize();
    tauri.invoke.mockClear();

    scan.handleDownloadCompleted("/music/Artist/Album");
    await vi.waitFor(() => {
      expect(tauri.invoke).toHaveBeenCalledWith("start_library_scan", {
        folderPath: "/music",
      });
    });
  });

  it("unsubscribes a listener that resolves after disposal", async () => {
    const unlisten = vi.fn();
    let finishListen: ((unlisten: () => void) => void) | undefined;
    tauri.listen.mockReturnValue(
      new Promise((resolve) => {
        finishListen = resolve;
      }),
    );
    const scan = useLibraryScan(vi.fn());

    const initializing = scan.initialize();
    scan.dispose();
    finishListen?.(unlisten);
    await initializing;

    expect(unlisten).toHaveBeenCalledOnce();
  });
});
