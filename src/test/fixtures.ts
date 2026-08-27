import type { AudioSourceRecord } from "../lib/audio-source-api";
import type { PluginRecord, RemoteTrack } from "../lib/plugin-api";
import type {
  LocalTrack,
  OnlineMusicSettings,
  OnlineTrack,
  OnlineTrackCandidate,
  ScanStatus,
} from "../generated/bindings";

export function createPluginRecord(
  overrides: Partial<PluginRecord> = {},
): PluginRecord {
  return {
    id: "fika.runtime-demo",
    name: "Fika Runtime Demo",
    version: "0.1.0",
    description: "Plugin navigation fixture",
    author: "Fika Music",
    path: "/plugins/runtime-demo",
    origin: "bundled",
    state: "disabled",
    enabled: false,
    permissionsReviewed: true,
    declaredCapabilities: [],
    grantedCapabilities: [],
    requiredHostBridges: [],
    providers: [
      {
        id: "fika-runtime-demo",
        entrypoint: "builtin:runtime-demo",
        initialized: false,
        sources: [],
        runtimeReport: null,
        diagnostics: [],
      },
    ],
    diagnostics: [],
    canRemove: false,
    canEnable: true,
    manifest: null,
    ...overrides,
  };
}

export function createAudioSourceRecord(
  overrides: Partial<AudioSourceRecord> = {},
): AudioSourceRecord {
  return {
    id: "imported-source",
    name: "Imported Source",
    version: "1.0.0",
    description: null,
    author: null,
    homepage: null,
    path: "/audio-sources/imported-source",
    adapter: "static-templates",
    state: "enabled",
    enabled: true,
    permissionsReviewed: true,
    declaredCapabilities: ["network:any"],
    grantedCapabilities: ["network:any"],
    sources: [
      {
        id: "wy",
        name: "NetEase",
        type: "music",
        actions: ["musicUrl"],
        qualities: ["128k", "320k"],
      },
    ],
    diagnostics: [],
    canRemove: true,
    canEnable: true,
    ...overrides,
  };
}

export function createOnlineMusicSettings(
  overrides: Partial<OnlineMusicSettings> = {},
): OnlineMusicSettings {
  return {
    excludedChannels: [],
    channelPriority: [],
    audioSourceSelectionMode: "automatic",
    audioSourcePriority: [],
    layerTimeoutSeconds: 8,
    playbackTimeoutSeconds: 20,
    playbackQuality: "320k",
    playbackCacheMaxMb: 500,
    downloadQuality: "320k",
    searchHistoryEnabled: true,
    downloadDirectory: null,
    filenameTemplate: "{artist} - {title}[ \\[{album}\\]]",
    downloadConcurrency: 2,
    batchNotifications: true,
    ...overrides,
  };
}

export function createScanStatus(overrides: Partial<ScanStatus> = {}): ScanStatus {
  return {
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
    ...overrides,
  };
}

export function createNeteaseTrack(overrides: Partial<RemoteTrack> = {}): RemoteTrack {
  return {
    id: "347230",
    source: "wy",
    title: "Test Track",
    artist: "Test Artist",
    album: null,
    durationSeconds: 180,
    coverUrl: null,
    rawInfo: { id: 347230 },
    ...overrides,
  };
}

export function createKugouTrack(overrides: Partial<RemoteTrack> = {}): RemoteTrack {
  const id = "4D766DEC7A90A011D730ED939D158131";
  return {
    id,
    source: "kg",
    title: "Under My Skin",
    artist: "Andrew Cui",
    album: "Under My Skin",
    durationSeconds: 205,
    coverUrl: null,
    rawInfo: { hash: id },
    ...overrides,
  };
}

export type TestSourceAccount = {
  accountRef: string;
  userId: string;
  displayName: string;
  avatarUrl: string | null;
  status: "active" | "expired";
  connectedAt: number;
  lastVerifiedAt: number;
};

export function createSourceAccount(
  overrides: Partial<TestSourceAccount> = {},
): TestSourceAccount {
  return {
    accountRef: "account:00000000-0000-4000-8000-000000000001",
    userId: "42",
    displayName: "Fika",
    avatarUrl: null,
    status: "active",
    connectedAt: 1,
    lastVerifiedAt: 1,
    ...overrides,
  };
}

export function createLocalTrack(overrides: Partial<LocalTrack> = {}): LocalTrack {
  return {
    id: 1,
    filePath: "/music/first.mp3",
    fileName: "first.mp3",
    title: "First",
    artist: "Artist",
    album: "Album",
    albumArtist: "Artist",
    genre: "Pop",
    year: 2024,
    codec: "MP3",
    bitrateKbps: 320,
    sampleRateHz: 44100,
    durationSeconds: 180,
    trackNumber: 1,
    discNumber: 1,
    fileSizeBytes: 1024,
    modifiedAt: 1,
    indexedAt: 1,
    playCount: 0,
    rating: 0,
    ...overrides,
  };
}

export function createOnlineTrackCandidate(
  overrides: Partial<OnlineTrackCandidate> = {},
): OnlineTrackCandidate {
  return {
    channelId: "netease",
    pluginId: "fika.netease",
    sourceId: "wy",
    channelName: "NetEase",
    id: "1",
    title: "Song",
    artist: "Artist",
    album: "Album",
    durationSeconds: 180,
    coverUrl: null,
    trackNumber: 1,
    discNumber: 1,
    platformIds: { id: "1" },
    rawInfo: {},
    rank: 1,
    ...overrides,
  };
}

export function createOnlineTrack(
  overrides: Partial<OnlineTrack> = {},
): OnlineTrack {
  return {
    key: "track",
    title: "Song",
    artist: "Artist",
    album: "Album",
    durationSeconds: 180,
    coverUrl: null,
    trackNumber: 1,
    discNumber: 1,
    candidates: [createOnlineTrackCandidate()],
    ...overrides,
  };
}
