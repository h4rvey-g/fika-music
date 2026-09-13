import { config, DOMWrapper, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";
import App from "./App.vue";
import type { PluginRecord } from "./lib/plugin-api";
import type { AudioSourceRecord } from "./lib/audio-source-api";
import type { OnlineTrack } from "./lib/online-music-api";
import type {
  LocalTrack,
  PlaybackMode,
  PlaybackQueueItem,
  PlaybackSessionSnapshot,
  PlaybackTrackInput,
} from "./generated/bindings";
import {
  COLLECTION_DRAG_TYPE,
  type MusicCollectionItem,
  type MusicCollectionSummary,
} from "./lib/collection-api";
import {
  THEME_GROUPS,
  THEME_MODE_OPTIONS,
  UI_PREFERENCES_STORAGE_KEY,
} from "./lib/ui-preferences";
import { DESKTOP_LYRICS_STORAGE_KEY } from "./lib/desktop-lyrics";
import { detectShortcutPlatform } from "./lib/keyboard-shortcuts";
import {
  DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
  GLOBAL_SHORTCUTS_STORAGE_KEY,
} from "./lib/global-shortcut-preferences";
import {
  NOW_PLAYING_LYRICS_SETTINGS_ID,
  NOW_PLAYING_LYRICS_STORAGE_KEY,
} from "./lib/now-playing-lyrics";
import {
  createAudioSourceRecord,
  createLocalTrack,
  createOnlineMusicSettings,
  createOnlineTrack,
  createOnlineTrackCandidate,
  createPluginRecord,
  createScanStatus,
} from "./test/fixtures";
import { createTestQueryPlugin } from "./test/query-client";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  emitTo: vi.fn(),
  getByLabel: vi.fn(),
}));

const dynamicThemeMocks = vi.hoisted(() => ({
  extractCoverTheme: vi.fn(),
}));

const collectionBrowserMocks = vi.hoisted(() => ({
  startCollection: vi.fn(),
}));

const libraryBrowserMocks = vi.hoisted(() => ({
  startRandomTrack: vi.fn(async () => undefined),
}));

const mediaSessionMocks = vi.hoisted(() => ({
  handlers: new Map<string, (() => void) | null>(),
  setActionHandler: vi.fn(),
}));

function platformModifier(): Pick<KeyboardEventInit, "ctrlKey" | "metaKey"> {
  return detectShortcutPlatform() === "mac" ? { metaKey: true } : { ctrlKey: true };
}

function dispatchShortcut(
  key: string,
  init: KeyboardEventInit = {},
  target: EventTarget = window,
) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

vi.mock("./lib/dynamic-theme", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/dynamic-theme")>();
  return {
    ...actual,
    extractCoverTheme: dynamicThemeMocks.extractCoverTheme,
  };
});

let listedPlugins: PluginRecord[] = [];
let listedAudioSources: AudioSourceRecord[] = [];
let listedCollections: MusicCollectionSummary[] = [];
let collectionBrowserPlaybackItems: MusicCollectionItem[] = [];
let onlineMusicSettings = createOnlineMusicSettings();
let mockPlaybackSession: PlaybackSessionSnapshot;
let mockPlaybackContext: PlaybackQueueItem[] = [];
let mockPlaybackHistory: PlaybackQueueItem[] = [];
let mockPlaybackSequence = 0;
let mockLocalTracks = new Map<number, LocalTrack>();

function emptyPlaybackSession(): PlaybackSessionSnapshot {
  return {
    revision: 0,
    mode: "sequential",
    current: null,
    upcoming: [],
    upcomingOffset: 0,
    upcomingTotal: 0,
    historyCount: 0,
    canGoPrevious: false,
    canGoNext: false,
    positionSeconds: 0,
    paused: true,
    streamOpen: false,
    consecutiveFailures: 0,
  };
}

function playbackItem(input: PlaybackTrackInput): PlaybackQueueItem {
  mockPlaybackSequence += 1;
  return input.kind === "online"
    ? { kind: "online", id: `event-${mockPlaybackSequence}`, track: input.track }
    : {
        kind: "local",
        id: `event-${mockPlaybackSequence}`,
        track: mockLocalTracks.get(input.trackId)
          ?? createLocalTrack({ id: input.trackId, title: `Track ${input.trackId}` }),
      };
}

function syncMockPlaybackSession() {
  mockPlaybackSession = {
    ...mockPlaybackSession,
    revision: mockPlaybackSession.revision + 1,
    upcomingTotal: mockPlaybackSession.upcoming.length,
    historyCount: mockPlaybackHistory.length,
    canGoPrevious: mockPlaybackHistory.length > 0,
    canGoNext: mockPlaybackSession.upcoming.length > 0
      || mockPlaybackSession.streamOpen
      || (mockPlaybackSession.mode === "repeat" && mockPlaybackContext.length > 0),
  };
  return structuredClone(mockPlaybackSession);
}

function replaceMockPlaybackSession(
  items: PlaybackQueueItem[],
  startIndex: number,
  autoplay: boolean,
  mode: PlaybackMode,
  streamOpen = false,
) {
  mockPlaybackContext = [...items];
  mockPlaybackHistory = [];
  const current = items[startIndex] ?? null;
  let upcoming = items.slice(startIndex + 1);
  if (mode === "repeat") upcoming = [...upcoming, ...items.slice(0, startIndex + 1)];
  mockPlaybackSession = {
    ...emptyPlaybackSession(),
    mode,
    current,
    upcoming,
    paused: !autoplay,
    streamOpen,
  };
  return syncMockPlaybackSession();
}

function localQueueItems(queueId: string) {
  if (queueId === "library-queue-from-first") {
    return [
      createLocalTrack({ id: 1, title: "Local Current" }),
      createLocalTrack({ id: 2, title: "Local Next" }),
      createLocalTrack({ id: 3, title: "Local Last" }),
    ];
  }
  if (queueId === "first-queue") return [mockLocalTracks.get(1) ?? createLocalTrack({ id: 1, title: "First" })];
  if (queueId === "second-queue") return [mockLocalTracks.get(2) ?? createLocalTrack({ id: 2, title: "Second" })];
  return [
    mockLocalTracks.get(1) ?? createLocalTrack({ id: 1, title: "Track 1" }),
    mockLocalTracks.get(2) ?? createLocalTrack({ id: 2, title: "Second" }),
  ];
}

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  isTauri: () => false,
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
  emitTo: tauriMocks.emitTo,
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: { getByLabel: tauriMocks.getByLabel },
}));

vi.mock("./components/PluginManager.vue", () => ({
  default: {
    name: "PluginManager",
    emits: ["pluginsChanged"],
    template: '<div data-testid="plugin-manager">Plugin manager</div>',
  },
}));

vi.mock("./components/AudioSourceManager.vue", () => ({
  default: {
    name: "AudioSourceManager",
    emits: ["sourcesChanged"],
    template: '<div data-testid="audio-source-manager">Audio source manager</div>',
  },
}));

vi.mock("./components/LibraryBrowser.vue", () => ({
  default: defineComponent({
    name: "LibraryBrowser",
    emits: ["playbackQueue", "queueTracks", "addToCollection", "createCollection", "summary", "error"],
    setup(_, { emit, expose }) {
      function playFirst() {
        emit(
          "playbackQueue",
          {
            queueId: "library-queue-from-first",
            total: 3,
            currentIndex: 0,
            track: createLocalTrack({ title: "Local Current" }),
          },
          true,
        );
      }
      function playSecond() {
        emit(
          "playbackQueue",
          {
            queueId: "library-queue",
            total: 2,
            currentIndex: 1,
            track: createLocalTrack({
              id: 2,
              filePath: "/music/second.mp3",
              fileName: "second.mp3",
              title: "Second",
              durationSeconds: 181,
              trackNumber: 2,
              fileSizeBytes: 2048,
            }),
          },
          true,
        );
      }
      expose({
        refresh: vi.fn(async () => undefined),
        startFirstTrack: vi.fn(async () => undefined),
        startRandomTrack: libraryBrowserMocks.startRandomTrack,
        updatePlayCount: vi.fn(),
      });
      return { playFirst, playSecond };
    },
    template: `<div>
      <button type="button" aria-label="Play First" @click="playFirst">Play first</button>
      <button type="button" aria-label="Play Second" @click="playSecond">Library browser</button>
    </div>`,
  }),
}));

vi.mock("./components/CollectionBrowser.vue", () => ({
  default: defineComponent({
    name: "CollectionBrowser",
    props: {
      collectionId: { type: String, required: true },
      refreshKey: { type: Number, required: true },
      activeLocalTrackId: { type: Number, default: null },
      activeOnlineTrack: { type: Object, default: null },
      isPlaying: { type: Boolean, required: true },
    },
    emits: ["play", "addToCollection", "createCollection", "changed", "error"],
    setup(_, { emit, expose }) {
      collectionBrowserMocks.startCollection.mockImplementation(async () => {
        if (collectionBrowserPlaybackItems.length) {
          emit("play", collectionBrowserPlaybackItems, 0, true);
        }
      });
      const methods = {
        refresh: vi.fn(async () => undefined),
        startCollection: collectionBrowserMocks.startCollection,
        startFirstTrack: vi.fn(async () => undefined),
        updatePlayCount: vi.fn(),
      };
      expose(methods);
      return methods;
    },
    template: '<div data-testid="collection-browser">Collection browser</div>',
  }),
}));

vi.mock("./components/NeteaseSource.vue", () => ({
  default: defineComponent({
    name: "NeteaseSource",
    props: {
      playbackSource: { type: String, required: true },
      audioSources: { type: Array, required: true },
    },
    emits: ["update:playbackSource", "openPlugins", "openAudioSources"],
    template: '<div data-testid="netease-source">NetEase source</div>',
  }),
}));

vi.mock("./components/KugouSource.vue", () => ({
  default: defineComponent({
    name: "KugouSource",
    props: {
      playbackSource: { type: String, required: true },
      audioSources: { type: Array, required: true },
    },
    emits: ["update:playbackSource", "openPlugins", "openAudioSources"],
    template: '<div data-testid="kugou-source">KuGou source</div>',
  }),
}));

describe("application shell", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listedPlugins = [];
    listedAudioSources = [];
    listedCollections = [];
    collectionBrowserPlaybackItems = [];
    onlineMusicSettings = createOnlineMusicSettings();
    mockPlaybackSession = emptyPlaybackSession();
    mockPlaybackContext = [];
    mockPlaybackHistory = [];
    mockPlaybackSequence = 0;
    mockLocalTracks = new Map([
      [1, createLocalTrack({ id: 1, title: "Track 1" })],
      [2, createLocalTrack({ id: 2, title: "Second" })],
      [3, createLocalTrack({ id: 3, title: "Local Last" })],
    ]);
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("style");
    dynamicThemeMocks.extractCoverTheme.mockResolvedValue(null);
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    mediaSessionMocks.handlers.clear();
    mediaSessionMocks.setActionHandler.mockImplementation(
      (action: string, handler: (() => void) | null) => {
        mediaSessionMocks.handlers.set(action, handler);
      },
    );
    Object.defineProperty(navigator, "mediaSession", {
      configurable: true,
      value: { setActionHandler: mediaSessionMocks.setActionHandler },
    });
    tauriMocks.listen.mockResolvedValue(vi.fn());
    tauriMocks.emitTo.mockResolvedValue(undefined);
    tauriMocks.getByLabel.mockResolvedValue(null);
    config.global.plugins = [createTestQueryPlugin()];
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_playback_session") {
        const limit = Number(args?.limit ?? mockPlaybackSession.upcoming.length);
        const offset = Number(args?.offset ?? 0);
        return Promise.resolve({
          ...structuredClone(mockPlaybackSession),
          upcoming: structuredClone(mockPlaybackSession.upcoming.slice(offset, offset + limit)),
          upcomingOffset: offset,
        });
      }
      if (command === "replace_playback_session") {
        const items = (args?.tracks as PlaybackTrackInput[]).map(playbackItem);
        return Promise.resolve(replaceMockPlaybackSession(
          items,
          Number(args?.startIndex ?? 0),
          Boolean(args?.autoplay),
          args?.mode as PlaybackMode,
          Boolean(args?.streamOpen),
        ));
      }
      if (command === "replace_local_playback_session") {
        const items = localQueueItems(String(args?.queueId)).map((track) => {
          mockPlaybackSequence += 1;
          return { kind: "local" as const, id: `event-${mockPlaybackSequence}`, track };
        });
        return Promise.resolve(replaceMockPlaybackSession(
          items,
          Number(args?.startIndex ?? 0),
          Boolean(args?.autoplay),
          args?.mode as PlaybackMode,
        ));
      }
      if (command === "play_next_in_playback_session") {
        const additions = (args?.tracks as PlaybackTrackInput[]).map(playbackItem);
        mockPlaybackSession.upcoming = [...additions, ...mockPlaybackSession.upcoming];
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "append_playback_session_stream") {
        mockPlaybackSession.upcoming.push(
          ...(args?.tracks as PlaybackTrackInput[]).map(playbackItem),
        );
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "close_playback_session_stream") {
        mockPlaybackSession.streamOpen = false;
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "set_playback_session_mode") {
        mockPlaybackSession.mode = args?.mode as PlaybackMode;
        if (
          mockPlaybackSession.mode === "repeat"
          && !mockPlaybackSession.upcoming.length
          && mockPlaybackContext.length
        ) {
          mockPlaybackSession.upcoming = [...mockPlaybackContext];
        }
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "move_playback_session_item") {
        const [item] = mockPlaybackSession.upcoming.splice(Number(args?.from), 1);
        if (item) mockPlaybackSession.upcoming.splice(Number(args?.to), 0, item);
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "remove_playback_session_item") {
        mockPlaybackSession.upcoming.splice(Number(args?.index), 1);
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "clear_playback_session_upcoming") {
        mockPlaybackSession.upcoming = [];
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "select_playback_session_item") {
        const selected = mockPlaybackSession.upcoming[Number(args?.index)];
        if (selected) {
          if (mockPlaybackSession.current) mockPlaybackHistory.push(mockPlaybackSession.current);
          mockPlaybackSession.current = selected;
          mockPlaybackSession.upcoming = mockPlaybackSession.upcoming.slice(Number(args?.index) + 1);
          mockPlaybackSession.positionSeconds = 0;
        }
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "next_playback_session_item") {
        if (!mockPlaybackSession.upcoming.length && mockPlaybackSession.mode === "repeat") {
          mockPlaybackSession.upcoming = [...mockPlaybackContext];
        }
        const next = mockPlaybackSession.upcoming.shift() ?? null;
        if (next) {
          if (mockPlaybackSession.current) mockPlaybackHistory.push(mockPlaybackSession.current);
          mockPlaybackSession.current = next;
          mockPlaybackSession.positionSeconds = 0;
        }
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "previous_playback_session_item") {
        const previous = mockPlaybackHistory.pop() ?? null;
        if (previous) {
          if (mockPlaybackSession.current) {
            mockPlaybackSession.upcoming.unshift(mockPlaybackSession.current);
          }
          mockPlaybackSession.current = previous;
          mockPlaybackSession.positionSeconds = 0;
        }
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "fail_playback_session_current") {
        mockPlaybackSession.consecutiveFailures += 1;
        if (mockPlaybackSession.consecutiveFailures < 3) {
          mockPlaybackSession.current = mockPlaybackSession.upcoming.shift() ?? null;
        }
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "mark_playback_session_started") {
        mockPlaybackSession.paused = false;
        mockPlaybackSession.consecutiveFailures = 0;
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "set_playback_session_paused") {
        mockPlaybackSession.paused = Boolean(args?.paused);
        return Promise.resolve(syncMockPlaybackSession());
      }
      if (command === "save_playback_session_progress") {
        mockPlaybackSession.positionSeconds = Number(args?.positionSeconds ?? 0);
        return Promise.resolve(null);
      }
      if (command === "get_scan_status") {
        return Promise.resolve(createScanStatus());
      }
      if (command === "list_plugins") {
        return Promise.resolve(listedPlugins);
      }
      if (command === "list_audio_sources") {
        return Promise.resolve(listedAudioSources);
      }
      if (command === "list_music_collections") {
        return Promise.resolve(listedCollections);
      }
      if (command === "create_music_collection") {
        const collection: MusicCollectionSummary = {
          id: `collection-${listedCollections.length + 1}`,
          name: String(args?.name ?? "Collection"),
          itemCount: 0,
          localCount: 0,
          onlineCount: 0,
          createdAt: 1,
          updatedAt: 1,
          smartRules: (args?.smartRules as MusicCollectionSummary["smartRules"]) ?? null,
        };
        listedCollections = [...listedCollections, collection];
        return Promise.resolve(collection);
      }
      if (command === "add_online_tracks_to_music_collection") {
        const collection = listedCollections.find(
          (candidate) => candidate.id === args?.collectionId,
        )!;
        const added = Array.isArray(args?.tracks) ? args.tracks.length : 0;
        const updated = { ...collection, itemCount: collection.itemCount + added };
        listedCollections = listedCollections.map((candidate) =>
          candidate.id === updated.id ? updated : candidate
        );
        return Promise.resolve({
          collection: updated,
          added,
          skipped: 0,
          removed: 0,
        });
      }
      if (command === "add_music_collection_items_to_music_collection") {
        const collection = listedCollections.find(
          (candidate) => candidate.id === args?.collectionId,
        )!;
        const added = Array.isArray(args?.itemIds) ? args.itemIds.length : 0;
        const updated = { ...collection, itemCount: collection.itemCount + added };
        listedCollections = listedCollections.map((candidate) =>
          candidate.id === updated.id ? updated : candidate
        );
        return Promise.resolve({
          collection: updated,
          added,
          skipped: 0,
          removed: 0,
        });
      }
      if (command === "get_online_music_settings") {
        return Promise.resolve(onlineMusicSettings);
      }
      if (command === "update_online_music_settings") {
        if (args?.settings) {
          onlineMusicSettings = args.settings as typeof onlineMusicSettings;
        }
        return Promise.resolve(onlineMusicSettings);
      }
      if (command === "list_online_download_tasks") {
        return Promise.resolve([]);
      }
      if (command === "list_online_music_channels") {
        return Promise.resolve([]);
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("style");
    config.global.plugins = [];
    Reflect.deleteProperty(navigator, "mediaSession");
    vi.restoreAllMocks();
  });

  it("navigates between all sidebar sections and closes the mobile drawer", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    expect(navigation.findAll("[data-section-id]").map((button) => button.text())).toEqual([
      "Local Music",
      "Online Music",
      "Audio Sources",
      "Plugins",
      "Settings",
    ]);
    expect(wrapper.get("h1").text()).toBe("Local Music");
    expect(wrapper.find("header p").exists()).toBe(false);
    expect(wrapper.get(".drawer-side aside").text()).not.toContain("Local-first library");
    expect(wrapper.get(".drawer-side aside").text()).not.toContain("No music folder");
    const playbackBar = wrapper.get('footer[aria-label="Playback bar"]');
    expect(playbackBar.text()).toContain("Nothing playing");

    const drawerToggle = wrapper.get<HTMLInputElement>("#app-sidebar");
    await drawerToggle.setValue(true);
    const settingsButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Settings");
    expect(settingsButton).toBeDefined();
    await settingsButton?.trigger("click");

    expect(wrapper.get("h1").text()).toBe("Settings");
    const settingsTabs = wrapper.get('[data-testid="settings-tabs"]');
    expect(settingsTabs.findAll('[role="tab"]').map((tab) => tab.text())).toEqual([
      "Appearance",
      "Playback",
      "Keyboard shortcuts",
      "Library",
      "Online Music",
      "Lyrics",
      "Software update",
    ]);
    expect(settingsTabs.get('[data-settings-tab="appearance"]').attributes("aria-selected"))
      .toBe("true");
    expect(settingsTabs.get('[data-settings-tab="appearance"]').attributes("aria-controls"))
      .toBe(wrapper.get('[role="tabpanel"]').attributes("id"));
    expect(wrapper.find("#theme-preference").exists()).toBe(true);
    expect(wrapper.find('[data-testid="app-update-settings"]').exists()).toBe(false);
    expect(wrapper.find("#layout-density").exists()).toBe(false);
    expect(wrapper.get('footer[aria-label="Playback bar"]').element).toBe(playbackBar.element);
    expect(settingsButton?.attributes("aria-current")).toBe("page");
    expect(drawerToggle.element.checked).toBe(false);

    await settingsTabs.get('[data-settings-tab="playback"]').trigger("click");
    expect(wrapper.find("#theme-preference").exists()).toBe(false);
    expect(wrapper.find("#default-volume").exists()).toBe(true);
    expect(wrapper.text()).not.toContain("System shortcuts are only available in the desktop app");

    await settingsTabs.get('[data-settings-tab="shortcuts"]').trigger("click");
    expect(wrapper.text()).toContain("Play or pause");
    expect(wrapper.text()).toContain("Open search");
    expect(wrapper.text()).toContain("System shortcuts");
    expect(wrapper.text()).toContain("System shortcuts are only available in the desktop app");

    await settingsTabs.get('[data-settings-tab="library"]').trigger("click");
    expect(wrapper.find("#default-volume").exists()).toBe(false);
    expect(wrapper.findAll("button").some((button) => button.text() === "Change folder")).toBe(true);

    await settingsTabs.get('[data-settings-tab="online"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="online-playback-quality"]').exists()).toBe(true);

    await settingsTabs.get('[data-settings-tab="lyrics"]').trigger("click");
    expect(wrapper.find(`#${NOW_PLAYING_LYRICS_SETTINGS_ID}`).exists()).toBe(true);

    await settingsTabs.get('[data-settings-tab="updates"]').trigger("click");
    expect(wrapper.find(`#${NOW_PLAYING_LYRICS_SETTINGS_ID}`).exists()).toBe(false);
    expect(wrapper.find('[data-testid="app-update-settings"]').exists()).toBe(true);

    await settingsTabs.get('[data-settings-tab="updates"]').trigger("keydown", { key: "ArrowLeft" });
    expect(settingsTabs.get('[data-settings-tab="lyrics"]').attributes("aria-selected"))
      .toBe("true");
    expect(wrapper.find(`#${NOW_PLAYING_LYRICS_SETTINGS_ID}`).exists()).toBe(true);

    const onlineButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Online Music");
    await onlineButton?.trigger("click");
    expect(wrapper.get("h1").text()).toBe("Online Music");
    expect(wrapper.get('input[aria-label="Search Online Music"]').attributes("placeholder"))
      .toBe("Search songs, artists, albums, and playlists");
    expect(wrapper.find('[aria-label="Now playing details"]').exists()).toBe(true);

    const sourcesButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Audio Sources");
    await sourcesButton?.trigger("click");
    expect(wrapper.find('[data-testid="audio-source-manager"]').exists()).toBe(true);
    expect(wrapper.findComponent({ name: "NowPlayingPanel" }).exists()).toBe(false);
    wrapper.unmount();
  });

  it("opens shortcut settings and handles application navigation shortcuts", async () => {
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    expect(navigation.findAll("button").some((button) => button.text().includes("Keyboard shortcuts")))
      .toBe(false);

    dispatchShortcut("/", platformModifier());
    await wrapper.vm.$nextTick();

    expect(wrapper.get("h1").text()).toBe("Settings");
    expect(wrapper.get('[data-settings-tab="shortcuts"]').attributes("aria-selected")).toBe("true");
    expect(wrapper.text()).toContain("Play or pause");
    expect(wrapper.text()).toContain("Open search");
    expect(wrapper.text()).toContain("System shortcuts");

    dispatchShortcut(",", platformModifier());
    await wrapper.vm.$nextTick();
    expect(wrapper.get("h1").text()).toBe("Settings");

    dispatchShortcut("k", platformModifier());
    await flushPromises();
    expect(wrapper.get("h1").text()).toBe("Online Music");
    expect(document.activeElement).toBe(wrapper.get('input[aria-label="Search Online Music"]').element);
    wrapper.unmount();
  });

  it("controls playback while leaving focused controls in charge of unmodified keys", async () => {
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: Record<string, unknown>) => {
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/second.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, payload);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const audio = wrapper.get("audio").element;
    Object.defineProperties(audio, {
      currentTime: { configurable: true, writable: true, value: 20 },
      duration: { configurable: true, value: 180 },
      paused: { configurable: true, writable: true, value: false },
    });
    audio.dispatchEvent(new Event("loadedmetadata"));
    await wrapper.vm.$nextTick();

    const pause = vi.mocked(HTMLMediaElement.prototype.pause);
    pause.mockClear();
    dispatchShortcut(" ");
    await wrapper.vm.$nextTick();
    expect(pause).toHaveBeenCalledOnce();

    dispatchShortcut("ArrowRight");
    await wrapper.vm.$nextTick();
    expect(audio.currentTime).toBe(25);

    dispatchShortcut("ArrowUp");
    await wrapper.vm.$nextTick();
    expect(wrapper.get<HTMLInputElement>('input[aria-label="Volume"]').element.value).toBe("0.85");

    dispatchShortcut("m");
    await wrapper.vm.$nextTick();
    expect(wrapper.find('button[aria-label="Unmute"]').exists()).toBe(true);

    pause.mockClear();
    Object.defineProperty(audio, "paused", { configurable: true, writable: true, value: false });
    const volumeControl = wrapper.get<HTMLInputElement>('input[aria-label="Volume"]');
    dispatchShortcut(" ", {}, volumeControl.element);
    await wrapper.vm.$nextTick();
    expect(pause).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("exposes persisted system shortcuts on commands and in shortcut settings", async () => {
    localStorage.setItem(GLOBAL_SHORTCUTS_STORAGE_KEY, JSON.stringify({
      ...DEFAULT_GLOBAL_SHORTCUT_PREFERENCES,
      toggleMute: "CommandOrControl+Shift+KeyM",
    }));
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();

    const systemAriaShortcut = detectShortcutPlatform() === "mac"
      ? "Meta+Shift+M"
      : "Control+Shift+M";
    expect(wrapper.get('button[aria-label="Mute"]').attributes("aria-keyshortcuts"))
      .toContain(systemAriaShortcut);

    dispatchShortcut("/", platformModifier());
    await wrapper.vm.$nextTick();
    expect(wrapper.get('[data-settings-tab="shortcuts"]').attributes("aria-selected")).toBe("true");
    expect(wrapper.get('button[aria-label*="Mute or unmute"]').attributes("aria-label"))
      .toContain("Current shortcut");
    wrapper.unmount();
  });

  it("switches the application to Simplified Chinese and persists the locale", async () => {
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('[data-section-id="settings"]').trigger("click");
    const languageSelect = wrapper.get<HTMLSelectElement>("#language-preference");
    expect(languageSelect.findAll("option").map((option) => option.text())).toEqual([
      "English",
      "简体中文",
    ]);

    await languageSelect.setValue("zh-CN");
    await wrapper.vm.$nextTick();

    expect(wrapper.get("h1").text()).toBe("设置");
    expect(
      wrapper
        .get('nav[aria-label="主导航"]')
        .findAll("[data-section-id]")
        .map((button) => button.text()),
    ).toEqual(["本地音乐", "在线音乐", "音频源", "插件", "设置"]);
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(JSON.parse(localStorage.getItem(UI_PREFERENCES_STORAGE_KEY) ?? "{}").locale)
      .toBe("zh-CN");

    wrapper.unmount();
  });

  it("expands Collections below Local Music and creates a named Collection", async () => {
    listedCollections = [{
      id: "collection-1",
      name: "Morning",
      itemCount: 3,
      localCount: 2,
      onlineCount: 1,
      createdAt: 1,
      updatedAt: 1,
      smartRules: null,
    }];
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const collection = navigation.get('[data-collection-id="collection-1"]');
    const online = navigation.get('[data-section-id="online"]');
    expect(
      collection.element.compareDocumentPosition(online.element)
      & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    await navigation.get('button[aria-label="Collapse Collections"]').trigger("click");
    expect(navigation.find('[data-collection-id="collection-1"]').exists()).toBe(false);
    await navigation.get('button[aria-label="New Collection"]').trigger("click");
    const dialog = new DOMWrapper(document.body.querySelector('[aria-labelledby="collection-name-dialog-title"]')!);
    await dialog.get('input[aria-label="Collection name"]').setValue("Road Trip");
    const create = dialog.findAll("button").find((button) => button.text() === "Create");
    await create?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_music_collection", {
      name: "Road Trip",
      smartRules: null,
    });
    expect(wrapper.get("h1").text()).toBe("Road Trip");
    expect(wrapper.find('[data-testid="collection-browser"]').exists()).toBe(true);
    expect(navigation.find('[data-collection-id="collection-2"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("creates a Smart Collection with text and numeric rules", async () => {
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('button[aria-label="New Collection"]').trigger("click");
    const dialog = new DOMWrapper(
      document.body.querySelector('[aria-labelledby="collection-name-dialog-title"]')!,
    );
    await dialog.get('input[aria-label="Collection name"]').setValue("孙燕姿 2000+");
    const smartMode = dialog.findAll("button").find((button) =>
      button.text().includes("Smart Collection"));
    await smartMode!.trigger("click");
    await dialog.get('input[aria-label="Rule 1 value"]').setValue("孙燕姿");
    const addRule = dialog.findAll("button").find((button) => button.text().includes("Add rule"));
    await addRule!.trigger("click");
    await dialog.get('select[aria-label="Rule 2 field"]').setValue("year");
    await dialog.get('select[aria-label="Rule 2 operator"]').setValue("greaterThan");
    await dialog.get('input[aria-label="Rule 2 value"]').setValue("2000");
    const create = dialog.findAll("button").find((button) => button.text() === "Create");
    await create!.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("create_music_collection", {
      name: "孙燕姿 2000+",
      smartRules: {
        rules: [
          { field: "artist", operator: "equals", value: "孙燕姿" },
          { field: "year", operator: "greaterThan", value: "2000" },
        ],
      },
    });
    expect(wrapper.find('[data-collection-id="collection-1"] svg').exists()).toBe(true);
    wrapper.unmount();
  });

  it("does not limit a Smart Collection to 32 rules", async () => {
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('button[aria-label="New Collection"]').trigger("click");
    const dialog = new DOMWrapper(
      document.body.querySelector('[aria-labelledby="collection-name-dialog-title"]')!,
    );
    const smartMode = dialog.findAll("button").find((button) =>
      button.text().includes("Smart Collection"));
    await smartMode!.trigger("click");
    const addRule = dialog.findAll("button").find((button) => button.text().includes("Add rule"));
    for (let index = 0; index < 32; index += 1) {
      await addRule!.trigger("click");
    }

    expect(dialog.findAll('select[aria-label$=" field"]')).toHaveLength(33);
    expect(addRule!.attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });

  it("starts a sidebar Collection from its first track on double-click", async () => {
    const collection: MusicCollectionSummary = {
      id: "collection-1",
      name: "Morning",
      itemCount: 1,
      localCount: 1,
      onlineCount: 0,
      createdAt: 1,
      updatedAt: 1,
      smartRules: null,
    };
    listedCollections = [collection];
    const track = createLocalTrack({ id: 7, title: "First Song" });
    mockLocalTracks.set(track.id, track);
    const item: MusicCollectionItem = {
      id: "item-1",
      position: 0,
      kind: "local",
      localTrack: track,
      localAlbumGroupId: "album:morning",
      onlineTrack: null,
      addedAt: 1,
    };
    collectionBrowserPlaybackItems = [item];
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/first.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, args);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('[data-collection-id="collection-1"]').trigger("dblclick");
    await flushPromises();

    expect(wrapper.get("h1").text()).toBe("Morning");
    expect(collectionBrowserMocks.startCollection).toHaveBeenCalledWith("collection-1");
    expect(tauriMocks.invoke).toHaveBeenCalledWith("local_track_media_source", { trackId: 7 });
    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("First Song");
    wrapper.unmount();
  });

  it("starts Local Music from a random track on sidebar double-click", async () => {
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('[data-section-id="settings"]').trigger("click");

    await wrapper.get('[data-section-id="local"]').trigger("dblclick");
    await flushPromises();

    expect(wrapper.get("h1").text()).toBe("Local Music");
    expect(libraryBrowserMocks.startRandomTrack).toHaveBeenCalledOnce();
    wrapper.unmount();
  });

  it("keeps the newest local track when media source requests resolve out of order", async () => {
    const firstTrack = createLocalTrack({ id: 1, title: "First" });
    const secondTrack = createLocalTrack({ id: 2, title: "Second" });
    mockLocalTracks.set(firstTrack.id, firstTrack);
    mockLocalTracks.set(secondTrack.id, secondTrack);
    let resolveFirstSource!: (source: { filePath: string }) => void;
    const firstSource = new Promise<{ filePath: string }>((resolve) => {
      resolveFirstSource = resolve;
    });
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "local_track_media_source") {
        return args?.trackId === firstTrack.id
          ? firstSource
          : Promise.resolve({ filePath: "/music/second.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, args);
    });
    const wrapper = mount(App);
    await flushPromises();
    const browser = wrapper.getComponent({ name: "LibraryBrowser" });

    browser.vm.$emit("playbackQueue", {
      queueId: "first-queue",
      total: 1,
      currentIndex: 0,
      track: firstTrack,
    }, true);
    await wrapper.vm.$nextTick();
    browser.vm.$emit("playbackQueue", {
      queueId: "second-queue",
      total: 1,
      currentIndex: 0,
      track: secondTrack,
    }, true);
    await flushPromises();

    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Second");
    expect(wrapper.get("audio").element.getAttribute("src")).toBe("/music/second.mp3");

    resolveFirstSource({ filePath: "/music/first.mp3" });
    await flushPromises();

    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Second");
    expect(wrapper.get("audio").element.getAttribute("src")).toBe("/music/second.mp3");
    wrapper.unmount();
  });

  it("opens New Collection from the Local Music context menu", async () => {
    const wrapper = mount(App);
    await flushPromises();

    await wrapper
      .get('nav[aria-label="Primary navigation"] [data-section-id="local"]')
      .trigger("contextmenu", { clientX: 80, clientY: 80 });

    const contextMenu = document.body.querySelector<HTMLElement>(
      '[data-sidebar-context-menu][aria-label="Local Music actions"]',
    );
    expect(contextMenu?.textContent).toContain("New Collection");
    contextMenu?.querySelector<HTMLButtonElement>("button")?.click();
    await wrapper.vm.$nextTick();
    expect(document.body.querySelector('input[aria-label="Collection name"]')).not.toBeNull();
    wrapper.unmount();
  });

  it("accepts dragged search results on a sidebar Collection", async () => {
    listedCollections = [{
      id: "collection-1",
      name: "Drop Target",
      itemCount: 0,
      localCount: 0,
      onlineCount: 0,
      createdAt: 1,
      updatedAt: 1,
      smartRules: null,
    }];
    const track = createOnlineTrack({ key: "drop-track" });
    const dataTransfer = {
      types: [COLLECTION_DRAG_TYPE],
      dropEffect: "none",
      getData: (type: string) => type === COLLECTION_DRAG_TYPE
        ? JSON.stringify({ kind: "online", tracks: [track] })
        : "",
    };
    const wrapper = mount(App);
    await flushPromises();
    const target = wrapper.get('[data-collection-id="collection-1"]');

    await target.trigger("dragover", { dataTransfer });
    expect(target.classes()).toContain("outline-primary");
    await target.trigger("drop", { dataTransfer });
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith(
      "add_online_tracks_to_music_collection",
      { collectionId: "collection-1", tracks: [track] },
    );
    expect(target.text()).toContain("1");
    wrapper.unmount();
  });

  it("copies a Collection selection through the Collection picker", async () => {
    listedCollections = [
      {
        id: "source",
        name: "Source",
        itemCount: 2,
        localCount: 1,
        onlineCount: 1,
        createdAt: 1,
        updatedAt: 1,
        smartRules: null,
      },
      {
        id: "target",
        name: "Target",
        itemCount: 0,
        localCount: 0,
        onlineCount: 0,
        createdAt: 2,
        updatedAt: 2,
        smartRules: null,
      },
    ];
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('[data-collection-id="source"]').trigger("click");
    const browser = wrapper.getComponent({ name: "CollectionBrowser" });

    browser.vm.$emit("addToCollection", {
      sourceCollectionId: "source",
      itemIds: ["item-1", "item-2"],
    });
    await wrapper.vm.$nextTick();
    const picker = new DOMWrapper(
      document.body.querySelector('[aria-labelledby="collection-picker-title"]')!,
    );
    expect(picker.text()).toContain("Target");
    expect(picker.text()).not.toContain("Source");
    const add = picker.findAll("button").find((button) => button.text() === "Add");
    await add?.trigger("click");
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith(
      "add_music_collection_items_to_music_collection",
      {
        collectionId: "target",
        sourceCollectionId: "source",
        itemIds: ["item-1", "item-2"],
      },
    );
    wrapper.unmount();
  });

  it("keeps a Collection queue pending until playback is requested", async () => {
    listedCollections = [{
      id: "collection-1",
      name: "Queue",
      itemCount: 1,
      localCount: 1,
      onlineCount: 0,
      createdAt: 1,
      updatedAt: 1,
      smartRules: null,
    }];
    const track = createLocalTrack({ id: 7, title: "Queued Song" });
    mockLocalTracks.set(track.id, track);
    const item: MusicCollectionItem = {
      id: "item-1",
      position: 0,
      kind: "local",
      localTrack: track,
      localAlbumGroupId: "album:queue",
      onlineTrack: null,
      addedAt: 1,
    };
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/queued.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, args);
    });
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('[data-collection-id="collection-1"]').trigger("click");

    wrapper.getComponent({ name: "CollectionBrowser" }).vm.$emit(
      "play",
      [item],
      0,
      false,
    );
    await flushPromises();
    expect(tauriMocks.invoke).not.toHaveBeenCalledWith(
      "local_track_media_source",
      expect.anything(),
    );

    await wrapper.get('button[aria-label="Play playback"]').trigger("click");
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("local_track_media_source", { trackId: 7 });
    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Queued Song");
    wrapper.unmount();
  });

  it("navigates view history with mouse back and forward buttons", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const selectSection = async (label: string) => {
      const button = navigation
        .findAll("button")
        .find((candidate) => candidate.text() === label);
      expect(button).toBeDefined();
      await button?.trigger("click");
    };
    const pressMouseButton = async (button: number) => {
      window.dispatchEvent(new MouseEvent("mousedown", { button, cancelable: true }));
      const mouseup = new MouseEvent("mouseup", { button, cancelable: true });
      window.dispatchEvent(mouseup);
      window.dispatchEvent(new MouseEvent("auxclick", { button, cancelable: true }));
      await wrapper.vm.$nextTick();
      expect(mouseup.defaultPrevented).toBe(true);
    };

    await selectSection("Settings");
    await selectSection("Online Music");
    expect(wrapper.get("h1").text()).toBe("Online Music");

    await pressMouseButton(3);
    expect(wrapper.get("h1").text()).toBe("Settings");
    await pressMouseButton(3);
    expect(wrapper.get("h1").text()).toBe("Local Music");
    await pressMouseButton(4);
    expect(wrapper.get("h1").text()).toBe("Settings");

    await selectSection("Audio Sources");
    await pressMouseButton(4);
    expect(wrapper.get("h1").text()).toBe("Audio Sources");
    wrapper.unmount();
  });

  it("opens and persists Now Playing lyric settings from the lyrics context menu", async () => {
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('[data-testid="lyrics-viewport"]').trigger("contextmenu", {
      clientX: 120,
      clientY: 120,
    });
    await wrapper.vm.$nextTick();

    const contextAction = document.body.querySelector<HTMLButtonElement>(
      "[data-lyrics-context-menu] button",
    );
    expect(contextAction?.textContent).toContain("Lyrics appearance");
    contextAction?.click();
    await flushPromises();

    expect(wrapper.get("h1").text()).toBe("Settings");
    const settings = wrapper.get(`#${NOW_PLAYING_LYRICS_SETTINGS_ID}`);
    expect(settings.attributes("tabindex")).toBe("-1");

    await settings.get('input[aria-label="Now playing lyric size"]').setValue("22");
    expect(
      JSON.parse(localStorage.getItem(NOW_PLAYING_LYRICS_STORAGE_KEY) ?? "{}").fontSize,
    ).toBe(22);

    const localButton = wrapper
      .get('nav[aria-label="Primary navigation"]')
      .findAll("button")
      .find((button) => button.text() === "Local Music");
    await localButton?.trigger("click");
    expect(
      wrapper.getComponent({ name: "NowPlayingPanel" }).props("lyricsPreferences"),
    ).toEqual(expect.objectContaining({ fontSize: 22 }));
    wrapper.unmount();
  });

  it("keeps folder selection without exposing a manual index action", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const header = wrapper.get("header");
    expect(header.text()).toContain("Folder");
    expect(header.text()).not.toContain("Index");

    wrapper.unmount();
  });

  it("prioritizes the Online Music workspace before pinning the navigation drawer", async () => {
    const wrapper = mount(App);
    await flushPromises();

    expect(wrapper.get(".drawer").classes()).toContain("min-[1200px]:drawer-open");
    expect(wrapper.get('label[aria-label="Open navigation"]').classes()).toContain(
      "min-[1200px]:hidden",
    );

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const onlineButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Online Music");
    await onlineButton?.trigger("click");

    const onlineMusic = wrapper.getComponent({ name: "OnlineMusic" });
    const onlineGrid = onlineMusic.element.parentElement;
    expect(onlineGrid).not.toBeNull();
    expect(onlineGrid?.classList).toContain(
      "min-[1000px]:grid-cols-[minmax(0,1fr)_20rem]",
    );
    expect(onlineMusic.classes()).toEqual(
      expect.arrayContaining([
        "min-[1000px]:col-start-1",
        "min-[1000px]:row-start-1",
      ]),
    );
    expect(wrapper.getComponent({ name: "NowPlayingPanel" }).classes()).toEqual(
      expect.arrayContaining([
        "min-[1000px]:sticky",
        "min-[1000px]:top-4",
        "min-[1000px]:col-start-2",
        "min-[1000px]:row-start-1",
      ]),
    );

    wrapper.unmount();
  });

  it("adds a dedicated sidebar entry and workspace for every enabled plugin", async () => {
    listedPlugins = [
      createPluginRecord({
        id: "fika.netease",
        name: "NetEase Cloud Music",
        state: "enabled",
        enabled: true,
      }),
      createPluginRecord({
        state: "enabled",
        enabled: true,
        providers: [
          {
            id: "fika-runtime-demo",
            entrypoint: "builtin:runtime-demo",
            initialized: true,
            sources: [
              {
                id: "demo",
                name: "Demo Music",
                type: "music",
                actions: ["musicSearch"],
                qualities: ["320k"],
              },
            ],
            runtimeReport: null,
            diagnostics: [],
          },
        ],
      }),
      createPluginRecord({ id: "fika.disabled", name: "Disabled Plugin" }),
    ];

    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const pluginButtons = navigation.findAll("button[data-plugin-id]");
    expect(pluginButtons.map((button) => button.text())).toEqual([
      "NetEase Cloud Music",
      "Fika Runtime Demo",
    ]);
    expect(navigation.text()).not.toContain("Disabled Plugin");

    await pluginButtons[0].trigger("click");
    expect(wrapper.get("h1").text()).toBe("NetEase Cloud Music");
    expect(wrapper.find('[data-testid="netease-source"]').exists()).toBe(true);
    expect(pluginButtons[0].attributes("aria-current")).toBe("page");

    await pluginButtons[1].trigger("click");
    expect(wrapper.get("h1").text()).toBe("Fika Runtime Demo");
    expect(wrapper.get('[data-testid="plugin-workspace"]').text()).toContain("Demo Music");
    expect(pluginButtons[1].attributes("aria-current")).toBe("page");
    wrapper.unmount();
  });

  it("preserves Online Music state while opening a dedicated plugin page", async () => {
    listedPlugins = [
      createPluginRecord({
        id: "fika.netease",
        name: "NetEase Cloud Music",
        state: "enabled",
        enabled: true,
      }),
    ];
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const onlineButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Online Music");
    await onlineButton?.trigger("click");
    await wrapper.get('input[aria-label="Search Online Music"]').setValue("M83");

    wrapper.getComponent({ name: "OnlineMusic" }).vm.$emit("openPlugin", "fika.netease");
    await wrapper.vm.$nextTick();
    expect(wrapper.get("h1").text()).toBe("NetEase Cloud Music");

    await onlineButton?.trigger("click");
    expect(wrapper.get<HTMLInputElement>('input[aria-label="Search Online Music"]').element.value)
      .toBe("M83");
    wrapper.unmount();
  });

  it("returns to the Online Music home when its active navigation item is clicked again", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    const onlineButton = navigation
      .findAll("button")
      .find((button) => button.text() === "Online Music");
    await onlineButton?.trigger("click");
    await wrapper.get('input[aria-label="Search Online Music"]').setValue("M83");
    await wrapper.get('form[role="search"]').trigger("submit");
    await flushPromises();

    expect(wrapper.find("[data-online-results]").exists()).toBe(true);
    await onlineButton?.trigger("click");
    await wrapper.vm.$nextTick();

    expect(wrapper.find("[data-online-home]").exists()).toBe(true);
    expect(wrapper.get<HTMLInputElement>('input[aria-label="Search Online Music"]').element.value)
      .toBe("");
    wrapper.unmount();
  });

  it("uses the shared audio source setting for NetEase", async () => {
    listedPlugins = [
      createPluginRecord({
        id: "fika.netease",
        name: "NetEase Cloud Music",
        state: "enabled",
        enabled: true,
      }),
    ];
    listedAudioSources = [
      createAudioSourceRecord({ id: "source-one", name: "Source One" }),
      createAudioSourceRecord({ id: "source-two", name: "Source Two" }),
    ];
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('button[data-plugin-id="fika.netease"]').trigger("click");
    const neteaseSource = wrapper.getComponent({ name: "NeteaseSource" });
    expect(neteaseSource.props("playbackSource")).toBe("source-one");
    expect(wrapper.findComponent({ name: "NowPlayingPanel" }).exists()).toBe(false);

    neteaseSource.vm.$emit("update:playbackSource", "source-two");
    await wrapper.vm.$nextTick();
    expect(neteaseSource.props("playbackSource")).toBe("source-two");
    expect(
      JSON.parse(localStorage.getItem(UI_PREFERENCES_STORAGE_KEY) ?? "{}").audioSourceId,
    ).toBe("source-two");

    wrapper.unmount();
  });

  it("closes the playback options menu after clicking outside it", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const menu = wrapper.get<HTMLDetailsElement>('[data-testid="playback-options-menu"]');
    menu.element.open = true;
    document.body.dispatchEvent(new MouseEvent("click", { bubbles: true }));

    expect(menu.element.open).toBe(false);
    wrapper.unmount();
  });

  it("changes the default audio source and quality from the playback options menu", async () => {
    onlineMusicSettings = createOnlineMusicSettings({
      audioSourceSelectionMode: "manual",
      playbackQuality: "128k",
    });
    listedAudioSources = [
      createAudioSourceRecord({ id: "source-one", name: "Source One" }),
      createAudioSourceRecord({ id: "source-two", name: "Source Two" }),
    ];
    const wrapper = mount(App);
    await flushPromises();

    const actions = wrapper.get('[data-testid="playback-actions"]');
    await actions.get('button[data-audio-source-id="source-two"]').trigger("click");
    await actions.get('button[data-stream-quality="320k"]').trigger("click");
    await flushPromises();

    expect(actions.get('button[data-audio-source-id="source-two"]').attributes("aria-checked"))
      .toBe("true");
    expect(actions.get('button[data-stream-quality="320k"]').attributes("aria-checked"))
      .toBe("true");
    expect(JSON.parse(localStorage.getItem(UI_PREFERENCES_STORAGE_KEY) ?? "{}"))
      .toEqual(expect.objectContaining({ audioSourceId: "source-two" }));
    expect(onlineMusicSettings.playbackQuality).toBe("320k");
    wrapper.unmount();
  });

  it("re-resolves the current online track after changing its source or quality", async () => {
    onlineMusicSettings = createOnlineMusicSettings({ audioSourceSelectionMode: "manual" });
    listedAudioSources = [
      createAudioSourceRecord({ id: "source-one", name: "Source One" }),
      createAudioSourceRecord({ id: "source-two", name: "Source Two" }),
    ];
    const track = createOnlineTrack();
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: {
      audioSourceId?: string;
      request?: { quality?: string };
    }) => {
      if (command === "list_online_music_channels") {
        return Promise.resolve([{
          id: "netease",
          pluginId: "fika.netease",
          pluginName: "NetEase",
          providerId: "netease",
          sourceId: "wy",
          sourceName: "NetEase",
          excluded: false,
          actions: ["musicUrl"],
        }]);
      }
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: {
            action: "musicUrl",
            data: `https://cdn.example.test/${payload?.audioSourceId}-${payload?.request?.quality}.mp3`,
          },
          diagnostics: [],
        });
      }
      if (command === "resolve_remote_track_lyrics") return Promise.resolve(null);
      return defaultInvoke?.(command, payload);
    });
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
    });
    const wrapper = mount(App);
    await flushPromises();

    wrapper
      .getComponent({ name: "OnlineMusic" })
      .vm.$emit("playRequest", track, [track], 0, true);
    await flushPromises();

    const trackInfo = wrapper.get('[data-testid="playback-track-info"]');
    expect(trackInfo.text()).toContain("Song");
    expect(trackInfo.text()).toContain("Artist - Album");
    expect(trackInfo.text()).not.toContain("Source One");

    const actions = wrapper.get('[data-testid="playback-actions"]');
    await actions.get('button[data-audio-source-id="source-two"]').trigger("click");
    await flushPromises();
    const latestPlaybackRequest = () => {
      const requests = tauriMocks.invoke.mock.calls
        .filter(([command]) => command === "dispatch_audio_source_request");
      return requests[requests.length - 1];
    };
    expect(latestPlaybackRequest()).toEqual([
      "dispatch_audio_source_request",
      expect.objectContaining({ audioSourceId: "source-two" }),
    ]);

    await actions.get('button[data-stream-quality="320k"]').trigger("click");
    await flushPromises();
    expect(latestPlaybackRequest()).toEqual([
      "dispatch_audio_source_request",
      expect.objectContaining({
        audioSourceId: "source-two",
        request: expect.objectContaining({ quality: "320k" }),
      }),
    ]);
    expect(actions.get('button[data-audio-source-id="source-two"]').attributes("aria-checked"))
      .toBe("true");
    expect(actions.get('button[data-stream-quality="320k"]').attributes("aria-checked"))
      .toBe("true");
    wrapper.unmount();
  });

  it("uses the winning online candidate identity for remote lyrics", async () => {
    listedAudioSources = [
      createAudioSourceRecord({
        id: "source-one",
        name: "Source One",
        sources: ["wy", "kg"].map((id) => ({
          id,
          name: id,
          type: "music",
          actions: ["musicUrl"],
          qualities: ["128k", "320k"],
        })),
      }),
    ];
    const onlineTrack = createOnlineTrack({
      key: "online-track",
      title: "Test Track",
      artist: "Test Artist",
      album: "Test Album",
      candidates: [
        createOnlineTrackCandidate({
          id: "347230",
          title: "Test Track",
          artist: "Test Artist",
          album: "Test Album",
          platformIds: { id: "347230" },
        }),
        createOnlineTrackCandidate({
          channelId: "kugou",
          pluginId: "fika.kugou",
          sourceId: "kg",
          channelName: "KuGou",
          id: "track-hash",
          title: "Test Track",
          artist: "Test Artist",
          album: "Test Album",
          platformIds: { hash: "track-hash" },
          rank: 2,
        }),
      ],
    });
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: { request?: { source?: string } }) => {
      if (command === "list_online_music_channels") {
        return Promise.resolve([
          {
            id: "netease",
            pluginId: "fika.netease",
            pluginName: "NetEase",
            providerId: "netease",
            sourceId: "wy",
            sourceName: "NetEase",
            excluded: false,
            actions: ["musicSearch"],
          },
          {
            id: "kugou",
            pluginId: "fika.kugou",
            pluginName: "KuGou",
            providerId: "kugou",
            sourceId: "kg",
            sourceName: "KuGou",
            excluded: false,
            actions: ["musicSearch"],
          },
        ]);
      }
      if (command === "dispatch_audio_source_request") {
        return payload?.request?.source === "wy"
          ? Promise.reject(new Error("NetEase URL failed"))
          : Promise.resolve({
              response: { action: "musicUrl", data: "https://cdn.example.test/track.mp3" },
              diagnostics: [],
            });
      }
      if (command === "resolve_remote_track_lyrics") return Promise.resolve(null);
      return defaultInvoke?.(command, payload);
    });
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
    });
    const wrapper = mount(App);
    await flushPromises();

    const onlineButton = wrapper
      .get('nav[aria-label="Primary navigation"]')
      .findAll("button")
      .find((button) => button.text() === "Online Music");
    await onlineButton?.trigger("click");
    wrapper
      .getComponent({ name: "OnlineMusic" })
      .vm.$emit("playRequest", onlineTrack, [onlineTrack], 0, true);
    await flushPromises();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("resolve_remote_track_lyrics", {
      query: expect.objectContaining({ source: "kg", trackId: "track-hash" }),
    });
    expect(wrapper.get("audio").attributes("type")).toBeUndefined();
    wrapper.unmount();
  });

  it("preloads and plays the next appendable online batch after reaching the queue end", async () => {
    listedAudioSources = [
      createAudioSourceRecord({
        id: "source-one",
        name: "Source One",
        sources: [{
          id: "wy",
          name: "NetEase",
          type: "music",
          actions: ["musicUrl"],
          qualities: ["128k", "320k"],
        }],
      }),
    ];
    const tracks = [1, 2, 3, 4].map((index) => createOnlineTrack({
      key: `roaming-${index}`,
      title: `Roaming ${index}`,
      candidates: [createOnlineTrackCandidate({
        id: String(index),
        title: `Roaming ${index}`,
        platformIds: { id: index },
      })],
    }));
    const queue = tracks.slice(0, 3);
    let resolveNextBatch!: () => void;
    const loadNext = vi.fn(() => new Promise<OnlineTrack[]>((resolve) => {
      resolveNextBatch = () => {
        queue.push(tracks[3]);
        resolve([tracks[3]]);
      };
    }));
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_online_music_channels") {
        return Promise.resolve([{
          id: "netease",
          pluginId: "fika.netease",
          pluginName: "NetEase",
          providerId: "netease",
          sourceId: "wy",
          sourceName: "NetEase",
          excluded: false,
          actions: ["musicUrl"],
        }]);
      }
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: "https://cdn.example.test/roaming.mp3" },
          diagnostics: [],
        });
      }
      if (command === "resolve_remote_track_lyrics") return Promise.resolve(null);
      return defaultInvoke?.(command, payload);
    });
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
    });
    const wrapper = mount(App);
    await flushPromises();

    wrapper
      .getComponent({ name: "OnlineMusic" })
      .vm.$emit("playRequest", tracks[2], queue, 2, true, loadNext);
    await flushPromises();

    expect(loadNext).toHaveBeenCalledTimes(1);
    expect(wrapper.get('footer[aria-label="Playback bar"]').text()).toContain("Roaming 3");

    wrapper.get("audio").element.dispatchEvent(new Event("ended"));
    await flushPromises();

    expect(loadNext).toHaveBeenCalledTimes(1);
    expect(wrapper.get('footer[aria-label="Playback bar"]').text()).toContain("Roaming 3");

    resolveNextBatch();
    await flushPromises();

    expect(wrapper.get('footer[aria-label="Playback bar"]').text()).toContain("Roaming 4");
    wrapper.unmount();
  });

  it("prepares the next online track and reuses it when playback ends", async () => {
    vi.useFakeTimers();
    listedAudioSources = [createAudioSourceRecord({ id: "source-one", name: "Source One" })];
    const tracks = [
      createOnlineTrack({ key: "track-one", title: "Track One" }),
      createOnlineTrack({
        key: "track-two",
        title: "Track Two",
        candidates: [createOnlineTrackCandidate({ id: "2", title: "Track Two" })],
      }),
    ];
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_online_music_channels") {
        return Promise.resolve([{
          id: "netease",
          pluginId: "fika.netease",
          pluginName: "NetEase",
          providerId: "netease",
          sourceId: "wy",
          sourceName: "NetEase",
          excluded: false,
          actions: ["musicUrl"],
        }]);
      }
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: "https://cdn.example.test/track.mp3" },
          diagnostics: [],
        });
      }
      if (command === "resolve_remote_track_lyrics") return Promise.resolve(null);
      return defaultInvoke?.(command, payload);
    });
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
    });
    const wrapper = mount(App);
    await flushPromises();

    wrapper.getComponent({ name: "OnlineMusic" }).vm.$emit(
      "playRequest",
      tracks[0],
      tracks,
      0,
      true,
    );
    await flushPromises();
    wrapper.get("audio").element.dispatchEvent(new Event("playing"));
    await vi.advanceTimersByTimeAsync(750);
    await flushPromises();

    const playbackRequests = () => tauriMocks.invoke.mock.calls
      .filter(([command]) => command === "dispatch_audio_source_request");
    expect(playbackRequests()).toHaveLength(2);

    wrapper.get("audio").element.dispatchEvent(new Event("ended"));
    await flushPromises();

    expect(playbackRequests()).toHaveLength(2);
    expect(wrapper.get('footer[aria-label="Playback bar"]').text()).toContain("Track Two");
    wrapper.unmount();
    vi.useRealTimers();
  });

  it("opens the dedicated KuGou workspace for the bundled plugin", async () => {
    listedPlugins = [
      createPluginRecord({
        id: "fika.kugou",
        name: "KuGou Music",
        state: "enabled",
        enabled: true,
      }),
    ];
    listedAudioSources = [
      createAudioSourceRecord({ id: "source-one", name: "Source One" }),
    ];
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('button[data-plugin-id="fika.kugou"]').trigger("click");

    const kugouSource = wrapper.getComponent({ name: "KugouSource" });
    expect(kugouSource.props("playbackSource")).toBe("source-one");
    expect(kugouSource.props("audioSources")).toContainEqual({
      value: "source-one",
      label: "Source One",
    });
    wrapper.unmount();
  });

  it("offers enabled standalone audio sources without adding Plugin entries", async () => {
    listedPlugins = [
      createPluginRecord({
        id: "fika.netease",
        name: "NetEase Cloud Music",
        state: "enabled",
        enabled: true,
      }),
    ];
    listedAudioSources = [
      createAudioSourceRecord({ id: "imported-lx-source", name: "Imported LX Source" }),
    ];
    const wrapper = mount(App);
    await flushPromises();

    await wrapper.get('button[data-plugin-id="fika.netease"]').trigger("click");

    expect(wrapper.getComponent({ name: "NeteaseSource" }).props("audioSources")).toContainEqual({
      value: "imported-lx-source",
      label: "Imported LX Source",
    });
    expect(wrapper.find('button[data-plugin-id="imported-lx-source"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("updates plugin sidebar entries when the plugin manager reports lifecycle changes", async () => {
    const plugin = createPluginRecord();
    listedPlugins = [plugin];
    const wrapper = mount(App);
    await flushPromises();

    expect(wrapper.find('button[data-plugin-id="fika.runtime-demo"]').exists()).toBe(false);
    const pluginsButton = wrapper
      .get('nav[aria-label="Primary navigation"]')
      .findAll("button")
      .find((button) => button.text() === "Plugins");
    await pluginsButton?.trigger("click");

    wrapper.getComponent({ name: "PluginManager" }).vm.$emit("pluginsChanged", [
      createPluginRecord({ state: "enabled", enabled: true }),
    ]);
    await wrapper.vm.$nextTick();

    expect(wrapper.get('button[data-plugin-id="fika.runtime-demo"]').text()).toBe(
      "Fika Runtime Demo",
    );
    wrapper.unmount();
  });

  it("offers, applies, and persists every configured daisyUI theme", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const settingsButton = wrapper
      .get('nav[aria-label="Primary navigation"]')
      .findAll("button")
      .find((button) => button.text() === "Settings");
    await settingsButton?.trigger("click");

    const themeSelect = wrapper.get<HTMLSelectElement>("#theme-preference");
    expect(
      themeSelect.findAll("optgroup").map((group) => ({
        label: group.attributes("label"),
        themes: group.findAll("option").map((option) => option.text()),
      })),
    ).toEqual(
      THEME_GROUPS.map((group) => ({
        label: group.label,
        themes: group.options.map((theme) => theme.label),
      })),
    );
    expect(themeSelect.findAll("option").map((option) => option.text())).toEqual(
      [
        ...THEME_MODE_OPTIONS.map((theme) => theme.label),
        ...THEME_GROUPS.flatMap((group) => group.options.map((theme) => theme.label)),
      ],
    );

    await themeSelect.setValue("dracula");

    expect(document.documentElement.dataset.theme).toBe("dracula");
    expect(JSON.parse(localStorage.getItem(UI_PREFERENCES_STORAGE_KEY) ?? "{}").theme).toBe(
      "dracula",
    );
    wrapper.unmount();
  });

  it("applies cover-derived DaisyUI colors in dynamic theme mode", async () => {
    const coverTheme = {
      base100: "rgb(244 248 253)",
      base200: "rgb(226 236 247)",
      base300: "rgb(198 217 237)",
      baseContent: "rgb(25 34 45)",
      primary: "rgb(22 88 166)",
      primaryContent: "rgb(255 255 255)",
      secondary: "rgb(74 61 174)",
      secondaryContent: "rgb(255 255 255)",
      accent: "rgb(18 151 140)",
      accentContent: "rgb(0 0 0)",
      neutral: "rgb(42 60 79)",
      neutralContent: "rgb(255 255 255)",
    };
    dynamicThemeMocks.extractCoverTheme.mockResolvedValue(coverTheme);
    localStorage.setItem(UI_PREFERENCES_STORAGE_KEY, JSON.stringify({ theme: "dynamic" }));
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_scan_status") return Promise.resolve(createScanStatus());
      if (command === "list_plugins") return Promise.resolve([]);
      if (command === "list_audio_sources") return Promise.resolve([]);
      if (command === "get_online_music_settings") {
        return Promise.resolve(onlineMusicSettings);
      }
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "list_online_music_channels") return Promise.resolve([]);
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/second.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({
          coverDataUrl: "data:image/png;base64,cover",
          lyrics: null,
          lyricsError: null,
        });
      }
      return defaultInvoke?.(command, args);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    expect(dynamicThemeMocks.extractCoverTheme).toHaveBeenCalledWith(
      "data:image/png;base64,cover",
    );
    expect(document.documentElement.dataset.theme).toBeUndefined();
    expect(document.documentElement.style.getPropertyValue("--color-base-100"))
      .toBe(coverTheme.base100);
    expect(document.documentElement.style.getPropertyValue("--color-primary"))
      .toBe(coverTheme.primary);
    expect(document.documentElement.style.getPropertyValue("--color-accent"))
      .toBe(coverTheme.accent);

    const settingsButton = wrapper
      .get('nav[aria-label="Primary navigation"]')
      .findAll("button")
      .find((button) => button.text() === "Settings");
    await settingsButton?.trigger("click");
    expect(wrapper.get('[role="status"]').text()).toBe("Cover colors active");

    dynamicThemeMocks.extractCoverTheme.mockResolvedValue(null);
    const themeSelect = wrapper.get<HTMLSelectElement>("#theme-preference");
    await themeSelect.setValue("system");
    await themeSelect.setValue("dynamic");
    await flushPromises();
    expect(wrapper.get('[role="status"]').text()).toBe("Cover colors unavailable");
    expect(document.documentElement.style.getPropertyValue("--color-base-100")).toBe("");

    wrapper.unmount();
    expect(document.documentElement.style.getPropertyValue("--color-primary")).toBe("");
  });

  it("cycles the playback mode from sequential to shuffle to repeat", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const modeButton = wrapper.get('[data-testid="playback-mode"]');
    expect(modeButton.attributes("aria-label")).toContain("Sequential");

    await modeButton.trigger("click");
    expect(modeButton.attributes("aria-label")).toContain("Shuffle");

    await modeButton.trigger("click");
    expect(modeButton.attributes("aria-label")).toContain("Repeat all");

    await modeButton.trigger("click");
    expect(modeButton.attributes("aria-label")).toContain("Sequential");
    wrapper.unmount();
  });

  it("toggles and persists the desktop lyrics window from the playback bar", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const toggle = wrapper.get('[data-testid="desktop-lyrics-toggle"]');
    expect(toggle.attributes("aria-pressed")).toBe("false");

    await toggle.trigger("click");

    expect(toggle.attributes("aria-pressed")).toBe("true");
    expect(
      JSON.parse(localStorage.getItem(DESKTOP_LYRICS_STORAGE_KEY) ?? "{}").enabled,
    ).toBe(true);
    wrapper.unmount();
  });

  it("broadcasts desktop word timing and freezes its clock while playback buffers", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    localStorage.setItem(DESKTOP_LYRICS_STORAGE_KEY, JSON.stringify({ enabled: true }));
    tauriMocks.getByLabel.mockResolvedValue({
      setMinSize: vi.fn().mockResolvedValue(undefined),
      setAlwaysOnTop: vi.fn().mockResolvedValue(undefined),
      setIgnoreCursorEvents: vi.fn().mockResolvedValue(undefined),
      show: vi.fn().mockResolvedValue(undefined),
      hide: vi.fn().mockResolvedValue(undefined),
    });
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_scan_status") return Promise.resolve(createScanStatus());
      if (command === "list_plugins") return Promise.resolve([]);
      if (command === "list_audio_sources") return Promise.resolve([]);
      if (command === "get_online_music_settings") {
        return Promise.resolve(onlineMusicSettings);
      }
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "list_online_music_channels") return Promise.resolve([]);
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/second.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({
          coverDataUrl: null,
          lyricsError: null,
          lyrics: {
            source: "sidecar",
            provider: null,
            isSynced: true,
            savedPath: null,
            matchScore: null,
            lines: [
              { startMs: 1_000, endMs: 3_000, text: "AB", words: [] },
              { startMs: 3_000, endMs: null, text: "Next", words: [] },
            ],
          },
        });
      }
      return defaultInvoke?.(command, args);
    });
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const audio = wrapper.get("audio").element;
    Object.defineProperty(audio, "currentTime", { configurable: true, value: 1.5 });
    Object.defineProperty(audio, "duration", { configurable: true, value: 10 });
    audio.dispatchEvent(new Event("timeupdate"));
    audio.dispatchEvent(new Event("waiting"));
    await wrapper.vm.$nextTick();

    expect(tauriMocks.emitTo).toHaveBeenLastCalledWith(
      "desktop-lyrics",
      "desktop-lyrics:state",
      expect.objectContaining({
        currentLine: "AB",
        currentTimingSource: "estimated",
        playbackPositionMs: 1_500,
        clockRunning: false,
        currentWords: [
          { text: "A", startMs: 1_000, endMs: 2_000 },
          { text: "B", startMs: 2_000, endMs: 3_000 },
        ],
      }),
    );
    wrapper.unmount();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("seeks the audio timeline from the now playing lyrics panel", async () => {
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_scan_status") return Promise.resolve(createScanStatus());
      if (command === "list_plugins") return Promise.resolve([]);
      if (command === "list_audio_sources") return Promise.resolve([]);
      if (command === "get_online_music_settings") {
        return Promise.resolve(onlineMusicSettings);
      }
      if (command === "list_online_download_tasks") return Promise.resolve([]);
      if (command === "list_online_music_channels") return Promise.resolve([]);
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: "/music/second.mp3" });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, args);
    });
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const audio = wrapper.get("audio").element;
    Object.defineProperties(audio, {
      currentTime: { configurable: true, writable: true, value: 0 },
      duration: { configurable: true, value: 180 },
    });
    audio.dispatchEvent(new Event("loadedmetadata"));
    await wrapper.vm.$nextTick();

    wrapper.getComponent({ name: "NowPlayingPanel" }).vm.$emit("seekPlayback", 42);
    await wrapper.vm.$nextTick();

    expect(audio.currentTime).toBe(42);
    expect(wrapper.get<HTMLInputElement>('input[aria-label="Seek playback"]').element.value)
      .toBe("42");
    wrapper.unmount();
  });

  it("navigates local tracks and wraps at the end in repeat mode", async () => {
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: Record<string, unknown>) => {
      if (command === "get_scan_status") {
        return Promise.resolve(createScanStatus({
          folderPath: "/music",
          discoveredFiles: 2,
          scannedFiles: 2,
          indexedTracks: 2,
        }));
      }
      if (command === "local_track_media_source") {
        return Promise.resolve({
          filePath: payload?.trackId === 2 ? "/music/second.mp3" : "/music/first.mp3",
        });
      }
      if (command === "local_library_queue_track") {
        return Promise.resolve({
          index: payload?.index ?? 0,
          track: createLocalTrack(),
        });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      if (command === "list_plugins") {
        return Promise.resolve(listedPlugins);
      }
      if (command === "list_audio_sources") {
        return Promise.resolve(listedAudioSources);
      }
      return defaultInvoke?.(command, payload);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const nextButton = wrapper.get('button[aria-label="Next track"]');
    expect(nextButton.attributes("disabled")).toBeDefined();

    const modeButton = wrapper.get('[data-testid="playback-mode"]');
    await modeButton.trigger("click");
    await modeButton.trigger("click");
    expect(nextButton.attributes("disabled")).toBeUndefined();

    await nextButton.trigger("click");
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("local_track_playback_details", {
      trackId: 1,
    });
    expect(wrapper.get('button[aria-label="Previous track"]').attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });

  it("plays queued tracks before resuming the current playback queue", async () => {
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: Record<string, unknown>) => {
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: `/music/${payload?.trackId ?? 1}.mp3` });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, payload);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const queuedTrack = createLocalTrack({ id: 1, title: "Queued First" });
    mockLocalTracks.set(queuedTrack.id, queuedTrack);
    wrapper.getComponent({ name: "LibraryBrowser" }).vm.$emit(
      "queueTracks",
      [queuedTrack],
    );
    await flushPromises();

    expect(wrapper.get('[data-testid="playback-queue-toggle"]').text()).toContain("1");
    await wrapper.get('[data-testid="playback-queue-toggle"]').trigger("click");
    expect(wrapper.get("#playback-queue-title").text()).toBe("Playback queue");
    expect(wrapper.text()).toContain("Queued First");

    wrapper.get("audio").element.dispatchEvent(new Event("ended"));
    await flushPromises();

    expect(wrapper.get('button[data-testid="playback-queue-toggle"]').text()).not.toContain("1");
    expect(wrapper.get('footer[aria-label="Playback bar"]').text()).toContain("Queued First");
    wrapper.unmount();
  });

  it("selects without playing and consumes the reordered local queue before double-click playback", async () => {
    const upcomingTracks = [
      { index: 1, track: createLocalTrack({ id: 2, title: "Local Next" }) },
      { index: 2, track: createLocalTrack({ id: 3, title: "Local Last" }) },
    ];
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: Record<string, unknown>) => {
      if (command === "local_library_queue_tracks") {
        return Promise.resolve(upcomingTracks);
      }
      if (command === "local_track_media_source") {
        return Promise.resolve({ filePath: `/music/${payload?.trackId ?? 1}.mp3` });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return defaultInvoke?.(command, payload);
    });

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play First"]').trigger("click");
    await flushPromises();
    await wrapper.get('[data-testid="playback-queue-toggle"]').trigger("click");

    expect(wrapper.findAll("[data-playback-queue-index]").map((row) => row.text()))
      .toEqual([
        expect.stringContaining("Local Next"),
        expect.stringContaining("Local Last"),
      ]);

    await wrapper.get('button[aria-label="Select Local Last"]').trigger("click");
    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Local Current");
    expect(tauriMocks.invoke).not.toHaveBeenCalledWith("select_playback_session_item", expect.anything());

    wrapper.getComponent({ name: "PlaybackQueue" }).vm.$emit("move", 1, 0);
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("move_playback_session_item", { from: 1, to: 0 });
    expect(wrapper.findAll("[data-playback-queue-index]").map((row) => row.text()))
      .toEqual([
        expect.stringContaining("Local Last"),
        expect.stringContaining("Local Next"),
      ]);

    await wrapper.get('button[aria-label="Next track"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Local Last");

    await wrapper.get('button[aria-label="Select Local Next"]').trigger("dblclick");
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("select_playback_session_item", { index: 0 });
    expect(wrapper.get('[data-testid="playback-track-info"]').text()).toContain("Local Next");
    wrapper.unmount();
  });

  it("shows the remaining remote tracks in the playback queue", async () => {
    listedAudioSources = [createAudioSourceRecord({ id: "source-one", name: "Source One" })];
    const tracks = [
      createOnlineTrack({ key: "remote-current", title: "Remote Current" }),
      createOnlineTrack({ key: "remote-next", title: "Remote Next" }),
      createOnlineTrack({ key: "remote-last", title: "Remote Last" }),
    ];
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_online_music_channels") {
        return Promise.resolve([{
          id: "netease",
          pluginId: "fika.netease",
          pluginName: "NetEase",
          providerId: "netease",
          sourceId: "wy",
          sourceName: "NetEase",
          excluded: false,
          actions: ["musicUrl"],
        }]);
      }
      if (command === "dispatch_audio_source_request") {
        return Promise.resolve({
          response: { action: "musicUrl", data: "https://cdn.example.test/track.mp3" },
          diagnostics: [],
        });
      }
      if (command === "resolve_remote_track_lyrics") return Promise.resolve(null);
      return defaultInvoke?.(command, payload);
    });
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (
      this: HTMLMediaElement,
    ) {
      queueMicrotask(() => this.dispatchEvent(new Event("canplay")));
    });

    const wrapper = mount(App);
    await flushPromises();
    wrapper.getComponent({ name: "OnlineMusic" }).vm.$emit(
      "playRequest",
      tracks[0],
      tracks,
      0,
      false,
    );
    await flushPromises();
    await wrapper.get('[data-testid="playback-queue-toggle"]').trigger("click");

    expect(wrapper.findAll("[data-playback-queue-index]").map((row) => row.text()))
      .toEqual([
        expect.stringContaining("Remote Next"),
        expect.stringContaining("Remote Last"),
      ]);
    wrapper.unmount();
  });

  it("loads a large playback queue in stable pages", async () => {
    mockPlaybackSession.upcoming = Array.from({ length: 250 }, (_, index) => ({
      kind: "local" as const,
      id: `large-${index}`,
      track: createLocalTrack({ id: index + 1, title: `Large ${index + 1}` }),
    }));
    syncMockPlaybackSession();
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('[data-testid="playback-queue-toggle"]').trigger("click");

    expect(wrapper.findAll("[data-playback-queue-index]")).toHaveLength(200);
    const loadMore = wrapper.findAll("button")
      .find((button) => button.text().includes("Load more"));
    await loadMore?.trigger("click");
    await flushPromises();

    expect(wrapper.findAll("[data-playback-queue-index]")).toHaveLength(250);
    expect(wrapper.findAll("[data-playback-queue-index]")[249]?.text()).toContain("Large 250");
    wrapper.unmount();
  });

  it("routes system previous and next media actions through the playback session", async () => {
    const defaultInvoke = tauriMocks.invoke.getMockImplementation();
    tauriMocks.invoke.mockImplementation(
      (command: string, payload?: Record<string, unknown>) => {
        if (command === "get_scan_status") {
          return Promise.resolve(createScanStatus({
            folderPath: "/music",
            discoveredFiles: 2,
            scannedFiles: 2,
            indexedTracks: 2,
          }));
        }
        if (command === "local_track_media_source") {
          return Promise.resolve({ filePath: `/music/${payload?.trackId ?? 1}.mp3` });
        }
        if (command === "local_track_playback_details") {
          return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
        }
        if (command === "list_plugins") return Promise.resolve(listedPlugins);
        if (command === "list_audio_sources") return Promise.resolve(listedAudioSources);
        if (command === "list_music_collections") return Promise.resolve(listedCollections);
        return defaultInvoke?.(command, payload);
      },
    );

    const wrapper = mount(App);
    await flushPromises();
    await wrapper.get('button[aria-label="Play Second"]').trigger("click");
    await flushPromises();

    const modeButton = wrapper.get('[data-testid="playback-mode"]');
    await modeButton.trigger("click");
    await flushPromises();
    await modeButton.trigger("click");
    await flushPromises();

    mediaSessionMocks.handlers.get("nexttrack")?.();
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("next_playback_session_item", {
      manual: true,
    });

    mediaSessionMocks.handlers.get("previoustrack")?.();
    await flushPromises();
    expect(tauriMocks.invoke).toHaveBeenCalledWith("previous_playback_session_item");

    wrapper.unmount();
    expect(mediaSessionMocks.handlers.get("previoustrack")).toBeNull();
    expect(mediaSessionMocks.handlers.get("nexttrack")).toBeNull();
  });
});
