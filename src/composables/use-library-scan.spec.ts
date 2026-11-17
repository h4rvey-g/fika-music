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

  it("loads the latest status after the progress subscription is ready", async () => {
    const running = createScanStatus({
      ...status,
      isRunning: true,
      discoveredFiles: 3480,
      scannedFiles: 320,
      indexedTracks: 320,
      finishedAt: null,
    });
    let backendStatus = running;
    let finishListen!: (unlisten: () => void) => void;
    tauri.listen.mockReturnValue(
      new Promise((resolve) => {
        finishListen = resolve;
      }),
    );
    tauri.invoke.mockImplementation(async () => backendStatus);
    const scan = useLibraryScan(vi.fn());

    const initializing = scan.initialize();
    await Promise.resolve();
    backendStatus = {
      ...running,
      isRunning: false,
      scannedFiles: 3480,
      indexedTracks: 3480,
      finishedAt: 2,
    };
    finishListen(vi.fn());
    await initializing;

    expect(scan.scanStatus.value).toEqual(backendStatus);
  });

  it("keeps completion progress when an older status request resolves later", async () => {
    let finishStatus!: (value: typeof status) => void;
    tauri.invoke.mockReturnValue(
      new Promise((resolve) => {
        finishStatus = resolve;
      }),
    );
    const scan = useLibraryScan(vi.fn());

    const initializing = scan.initialize();
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalled());
    const onProgress = tauri.listen.mock.calls[0][1];
    onProgress({ payload: { status, message: "Finished indexing local tracks." } });
    finishStatus({ ...status, isRunning: true, scannedFiles: 1, finishedAt: null });
    await initializing;

    expect(scan.scanStatus.value).toEqual(status);
    expect(scan.selectedFolder.value).toBe("/music");
  });

  it("starts indexing immediately after a folder is selected", async () => {
    const scan = useLibraryScan(vi.fn());
    await scan.initialize();
    tauri.invoke.mockClear();
    tauri.invoke.mockResolvedValueOnce("/new-music").mockResolvedValueOnce({
      ...status,
      folderPath: "/new-music",
      isRunning: true,
    });

    await scan.chooseFolder();

    expect(tauri.invoke).toHaveBeenNthCalledWith(1, "select_music_folder");
    expect(tauri.invoke).toHaveBeenNthCalledWith(2, "start_library_scan", {
      folderPath: "/new-music",
    });
  });

  it("keeps completion progress when the start response resolves later", async () => {
    const scan = useLibraryScan(vi.fn());
    await scan.initialize();
    tauri.invoke.mockClear();
    const completed = { ...status, startedAt: 3, finishedAt: 4 };
    let finishStart!: (value: typeof status) => void;
    tauri.invoke.mockResolvedValueOnce("/music").mockReturnValueOnce(
      new Promise((resolve) => {
        finishStart = resolve;
      }),
    );

    const choosing = scan.chooseFolder();
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledTimes(2));
    const onProgress = tauri.listen.mock.calls[0][1];
    onProgress({ payload: { status: completed, message: "Finished indexing local tracks." } });
    finishStart({ ...completed, isRunning: true, scannedFiles: 0, finishedAt: null });
    await choosing;

    expect(scan.scanStatus.value).toEqual(completed);
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
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});
