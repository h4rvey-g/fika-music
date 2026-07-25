import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { TAURI_COMMANDS } from "../generated/bindings";
import type { ScanProgressEvent, ScanStatus } from "../generated/bindings";
import { normalizeError } from "../lib/errors";

const emptyScanStatus: ScanStatus = {
  isRunning: false,
  folderPath: null,
  discoveredFiles: 0,
  scannedFiles: 0,
  indexedTracks: 0,
  skippedFiles: 0,
  errorCount: 0,
  lastError: null,
  startedAt: null,
  finishedAt: null,
};

export function useLibraryScan(updateError: (message: string | null) => void) {
  const scanStatus = ref<ScanStatus>({ ...emptyScanStatus });
  const selectedFolder = ref<string | null>(null);
  const scanMessage = ref<string | null>(null);
  const isChoosingFolder = ref(false);
  const isStartingScan = ref(false);
  const canScan = computed(
    () => Boolean(selectedFolder.value) && !scanStatus.value.isRunning && !isStartingScan.value,
  );
  let unlistenScanProgress: UnlistenFn | null = null;
  let disposed = false;

  async function initialize(): Promise<void> {
    disposed = false;
    await Promise.all([loadStatus(), bindProgress()]);
  }

  async function loadStatus(): Promise<void> {
    try {
      scanStatus.value = await invoke<ScanStatus>(TAURI_COMMANDS.getScanStatus);
      selectedFolder.value = scanStatus.value.folderPath;
    } catch (error) {
      updateError(normalizeError(error));
    }
  }

  async function bindProgress(): Promise<void> {
    try {
      const unlisten = await listen<ScanProgressEvent>(
        "library:scan-progress",
        (event) => {
          scanStatus.value = event.payload.status;
          scanMessage.value = event.payload.message;
          if (
            !event.payload.status.isRunning &&
            event.payload.message?.startsWith("Indexing failed:")
          ) {
            updateError(event.payload.message);
          }
        },
      );
      if (disposed) {
        unlisten();
      } else {
        unlistenScanProgress = unlisten;
      }
    } catch (error) {
      updateError(normalizeError(error));
    }
  }

  async function chooseFolder(): Promise<void> {
    isChoosingFolder.value = true;
    updateError(null);
    try {
      const folder = await invoke<string | null>(TAURI_COMMANDS.selectMusicFolder);
      if (folder) selectedFolder.value = folder;
    } catch (error) {
      updateError(normalizeError(error));
    } finally {
      isChoosingFolder.value = false;
    }
  }

  async function startScan(): Promise<void> {
    if (!selectedFolder.value) return;
    isStartingScan.value = true;
    updateError(null);
    scanMessage.value = null;
    try {
      scanStatus.value = await invoke<ScanStatus>(TAURI_COMMANDS.startLibraryScan, {
        folderPath: selectedFolder.value,
      });
    } catch (error) {
      updateError(normalizeError(error));
    } finally {
      isStartingScan.value = false;
    }
  }

  function handleDownloadCompleted(destination: string): void {
    if (!selectedFolder.value || scanStatus.value.isRunning) return;
    const normalize = (path: string) => path.replace(/[\\/]+$/, "");
    const library = normalize(selectedFolder.value);
    const downloaded = normalize(destination);
    if (
      downloaded === library ||
      downloaded.startsWith(`${library}/`) ||
      downloaded.startsWith(`${library}\\`)
    ) {
      void startScan();
    }
  }

  function dispose(): void {
    disposed = true;
    unlistenScanProgress?.();
    unlistenScanProgress = null;
  }

  return {
    scanStatus,
    selectedFolder,
    scanMessage,
    isChoosingFolder,
    isStartingScan,
    canScan,
    initialize,
    chooseFolder,
    startScan,
    handleDownloadCompleted,
    dispose,
  };
}
