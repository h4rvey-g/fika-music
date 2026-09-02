import { describe, expect, it, vi } from "vitest";
import {
  useAppUpdater,
  type AppUpdateResource,
  type AppUpdaterDependencies,
} from "./use-app-updater";

function createUpdate(
  overrides: Partial<AppUpdateResource> = {},
): AppUpdateResource {
  return {
    currentVersion: "0.1.1",
    version: "0.2.0",
    date: "2026-08-07T12:00:00Z",
    body: "A focused release.",
    installMode: "native",
    downloadAndInstall: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    ...overrides,
  };
}

function createDependencies(
  overrides: Partial<AppUpdaterDependencies> = {},
): AppUpdaterDependencies {
  return {
    isTauri: () => true,
    getVersion: vi.fn(async () => "0.1.1"),
    platform: () => "linux",
    check: vi.fn(async () => null),
    checkAndroid: vi.fn(async () => null),
    relaunch: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("useAppUpdater", () => {
  it("uses the Android release checker instead of the unsupported native updater", async () => {
    const update = createUpdate({ installMode: "external" });
    const nativeCheck = vi.fn(async () => {
      throw new Error("unsupported OS");
    });
    const checkAndroid = vi.fn(async () => update);
    const dependencies = createDependencies({
      platform: () => "android",
      check: nativeCheck,
      checkAndroid,
    });
    const updater = useAppUpdater(dependencies);

    await updater.initialize();

    expect(nativeCheck).not.toHaveBeenCalled();
    expect(checkAndroid).toHaveBeenCalledWith("0.1.1");
    expect(updater.error.value).toBeNull();
  });

  it("checks on initialization and exposes an available update", async () => {
    const update = createUpdate();
    const dependencies = createDependencies({
      check: vi.fn(async () => update),
    });
    const updater = useAppUpdater(dependencies);

    await updater.initialize();

    expect(dependencies.check).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(updater.availableUpdate.value).toEqual({
      currentVersion: "0.1.1",
      version: "0.2.0",
      date: "2026-08-07T12:00:00Z",
      body: "A focused release.",
      installMode: "native",
    });
    expect(updater.notificationVisible.value).toBe(true);
  });

  it("keeps automatic check failures quiet but reports manual failures", async () => {
    const dependencies = createDependencies({
      check: vi.fn(async () => {
        throw new Error("release endpoint unavailable");
      }),
    });
    const updater = useAppUpdater(dependencies);

    await updater.initialize();
    expect(updater.error.value).toBeNull();

    await updater.checkForUpdates();
    expect(updater.error.value).toBe("release endpoint unavailable");
  });

  it("downloads, installs, and relaunches with progress", async () => {
    const downloadAndInstall = vi.fn<AppUpdateResource["downloadAndInstall"]>(
      async (onEvent, options) => {
        expect(options).toEqual({ timeout: 600_000 });
        onEvent?.({ event: "Started", data: { contentLength: 100 } });
        onEvent?.({ event: "Progress", data: { chunkLength: 40 } });
        onEvent?.({ event: "Progress", data: { chunkLength: 60 } });
        onEvent?.({ event: "Finished" });
      },
    );
    const update = createUpdate({ downloadAndInstall });
    const dependencies = createDependencies({
      check: vi.fn(async () => update),
    });
    const updater = useAppUpdater(dependencies);
    await updater.initialize();

    await updater.installUpdate();

    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(updater.downloadedBytes.value).toBe(100);
    expect(updater.downloadPercent.value).toBe(100);
    expect(updater.restartRequired.value).toBe(true);
    expect(dependencies.relaunch).toHaveBeenCalledOnce();
  });

  it("opens an Android APK externally without relaunching", async () => {
    const downloadAndInstall = vi.fn(async () => undefined);
    const update = createUpdate({
      installMode: "external",
      downloadAndInstall,
    });
    const dependencies = createDependencies({
      platform: () => "android",
      checkAndroid: vi.fn(async () => update),
    });
    const updater = useAppUpdater(dependencies);
    await updater.initialize();

    await updater.installUpdate();

    expect(downloadAndInstall).toHaveBeenCalledWith();
    expect(updater.restartRequired.value).toBe(false);
    expect(dependencies.relaunch).not.toHaveBeenCalled();
  });
});
