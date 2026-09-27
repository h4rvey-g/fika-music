import { computed, ref, shallowRef } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { arch, platform } from "@tauri-apps/plugin-os";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check,
  type CheckOptions,
  type DownloadEvent,
  type DownloadOptions,
} from "@tauri-apps/plugin-updater";
import { checkAndroidUpdate } from "../lib/android-update";
import { normalizeError } from "../lib/errors";

const UPDATE_CHECK_TIMEOUT_MS = 30_000;
const UPDATE_DOWNLOAD_TIMEOUT_MS = 10 * 60_000;

export type AppUpdateSummary = {
  currentVersion: string;
  version: string;
  date: string | null;
  body: string | null;
  installMode: AppUpdateInstallMode;
};

export type AppUpdateInstallMode = "native" | "external";

export type AppUpdateResource = {
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
  installMode?: AppUpdateInstallMode;
  downloadAndInstall: (
    onEvent?: (event: DownloadEvent) => void,
    options?: DownloadOptions,
  ) => Promise<void>;
  close: () => Promise<void>;
};

export type AppUpdaterDependencies = {
  isTauri: () => boolean;
  getVersion: () => Promise<string>;
  platform: () => string;
  check: (options?: CheckOptions) => Promise<AppUpdateResource | null>;
  checkAndroid: (currentVersion: string) => Promise<AppUpdateResource | null>;
  relaunch: () => Promise<void>;
};

async function checkDefaultAndroidUpdate(
  currentVersion: string,
): Promise<AppUpdateResource | null> {
  const update = await checkAndroidUpdate({
    currentVersion,
    architecture: arch(),
  });
  if (!update) return null;

  return {
    currentVersion,
    version: update.version,
    date: update.date ?? undefined,
    body: update.body ?? undefined,
    installMode: "external",
    downloadAndInstall: async () => openUrl(update.downloadUrl),
    close: async () => undefined,
  };
}

const defaultDependencies: AppUpdaterDependencies = {
  isTauri,
  getVersion,
  platform,
  check,
  checkAndroid: checkDefaultAndroidUpdate,
  relaunch,
};

export function useAppUpdater(
  dependencies: AppUpdaterDependencies = defaultDependencies,
) {
  const currentVersion = ref<string | null>(null);
  const availableUpdate = ref<AppUpdateSummary | null>(null);
  const isChecking = ref(false);
  const isInstalling = ref(false);
  const hasChecked = ref(false);
  const restartRequired = ref(false);
  const downloadedBytes = ref(0);
  const totalBytes = ref<number | null>(null);
  const error = ref<string | null>(null);
  const notificationDismissed = ref(false);
  const updateResource = shallowRef<AppUpdateResource | null>(null);
  let checkPromise: Promise<void> | null = null;
  let installPromise: Promise<void> | null = null;
  let disposed = false;

  const downloadPercent = computed(() => {
    const total = totalBytes.value;
    if (!total || total <= 0) return 0;
    return Math.min(100, Math.round((downloadedBytes.value / total) * 100));
  });
  const notificationVisible = computed(
    () => Boolean(availableUpdate.value) && !notificationDismissed.value,
  );

  async function initialize(): Promise<void> {
    disposed = false;
    if (!dependencies.isTauri()) return;

    try {
      currentVersion.value = await dependencies.getVersion();
    } catch {
      currentVersion.value = null;
    }

    await checkForUpdates({ automatic: true });
  }

  function checkForUpdates(options: { automatic?: boolean } = {}): Promise<void> {
    if (checkPromise) return checkPromise;
    if (isInstalling.value || restartRequired.value) return Promise.resolve();

    checkPromise = performUpdateCheck(options.automatic ?? false).finally(() => {
      checkPromise = null;
    });
    return checkPromise;
  }

  async function performUpdateCheck(automatic: boolean): Promise<void> {
    if (!dependencies.isTauri()) {
      if (!automatic) {
        error.value = "Updates are only available in the desktop app.";
      }
      return;
    }

    isChecking.value = true;
    error.value = null;
    try {
      const nextUpdate = dependencies.platform() === "android"
        ? await checkForAndroidUpdate()
        : await dependencies.check({ timeout: UPDATE_CHECK_TIMEOUT_MS });
      if (disposed) {
        await closeUpdate(nextUpdate);
        return;
      }

      const previousUpdate = updateResource.value;
      updateResource.value = nextUpdate;
      await closeUpdate(previousUpdate);
      hasChecked.value = true;
      downloadedBytes.value = 0;
      totalBytes.value = null;

      if (!nextUpdate) {
        availableUpdate.value = null;
        return;
      }

      currentVersion.value = nextUpdate.currentVersion;
      availableUpdate.value = {
        currentVersion: nextUpdate.currentVersion,
        version: nextUpdate.version,
        date: nextUpdate.date ?? null,
        body: nextUpdate.body ?? null,
        installMode: nextUpdate.installMode ?? "native",
      };
      notificationDismissed.value = false;
    } catch (checkError) {
      if (!automatic && !disposed) {
        error.value = normalizeError(checkError, "Unable to check for updates.");
      }
    } finally {
      if (!disposed) isChecking.value = false;
    }
  }

  async function checkForAndroidUpdate(): Promise<AppUpdateResource | null> {
    if (!currentVersion.value) {
      try {
        currentVersion.value = await dependencies.getVersion();
      } catch {
        throw new Error("Unable to read the installed Android app version.");
      }
    }
    const version = currentVersion.value;
    if (!version) throw new Error("Unable to read the installed Android app version.");
    return dependencies.checkAndroid(version);
  }

  function installUpdate(): Promise<void> {
    if (installPromise) return installPromise;
    const update = updateResource.value;
    if (!update || restartRequired.value) return Promise.resolve();

    installPromise = performUpdateInstall(update).finally(() => {
      installPromise = null;
    });
    return installPromise;
  }

  async function performUpdateInstall(update: AppUpdateResource): Promise<void> {
    isInstalling.value = true;
    downloadedBytes.value = 0;
    totalBytes.value = null;
    error.value = null;

    try {
      if (update.installMode === "external") {
        await update.downloadAndInstall();
        return;
      }
      await update.downloadAndInstall(handleDownloadEvent, {
        timeout: UPDATE_DOWNLOAD_TIMEOUT_MS,
      });
      restartRequired.value = true;
      await restartApp();
    } catch (installError) {
      if (!disposed) {
        error.value = normalizeError(installError, "Unable to install the update.");
      }
    } finally {
      if (!disposed) isInstalling.value = false;
    }
  }

  function handleDownloadEvent(event: DownloadEvent): void {
    if (event.event === "Started") {
      totalBytes.value = event.data.contentLength ?? null;
      return;
    }
    if (event.event === "Progress") {
      downloadedBytes.value += event.data.chunkLength;
      return;
    }
    if (totalBytes.value !== null) {
      downloadedBytes.value = totalBytes.value;
    }
  }

  async function restartApp(): Promise<void> {
    error.value = null;
    try {
      await dependencies.relaunch();
    } catch (restartError) {
      if (!disposed) {
        error.value = normalizeError(restartError, "Unable to restart Fika Music.");
      }
    }
  }

  function dismissNotification(): void {
    notificationDismissed.value = true;
  }

  function dispose(): void {
    disposed = true;
    const update = updateResource.value;
    updateResource.value = null;
    if (!isInstalling.value) void closeUpdate(update);
  }

  async function closeUpdate(update: AppUpdateResource | null): Promise<void> {
    if (!update) return;
    try {
      await update.close();
    } catch {
      // The native resource may already be consumed after installation.
    }
  }

  return {
    currentVersion,
    availableUpdate,
    isChecking,
    isInstalling,
    hasChecked,
    restartRequired,
    downloadedBytes,
    totalBytes,
    downloadPercent,
    error,
    notificationVisible,
    initialize,
    checkForUpdates,
    installUpdate,
    restartApp,
    dismissNotification,
    dispose,
  };
}
