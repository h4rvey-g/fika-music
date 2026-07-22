import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";
import App from "./App.vue";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
}));

vi.mock("./components/PluginManager.vue", () => ({
  default: {
    name: "PluginManager",
    template: '<div data-testid="plugin-manager">Plugin manager</div>',
  },
}));

vi.mock("./components/LibraryBrowser.vue", () => ({
  default: defineComponent({
    name: "LibraryBrowser",
    emits: ["playbackQueue", "summary", "error", "index"],
    setup(_, { emit }) {
      function playSecond() {
        emit(
          "playbackQueue",
          {
            queueId: "library-queue",
            total: 2,
            currentIndex: 1,
            track: {
              id: 2,
              filePath: "/music/second.mp3",
              fileName: "second.mp3",
              title: "Second",
              artist: "Artist",
              album: "Album",
              albumArtist: "Artist",
              genre: "Pop",
              year: 2024,
              codec: "MP3",
              bitrateKbps: 320,
              sampleRateHz: 44100,
              durationSeconds: 181,
              trackNumber: 2,
              discNumber: 1,
              fileSizeBytes: 2048,
              modifiedAt: 1,
              indexedAt: 1,
              playCount: 0,
            },
          },
          true,
        );
      }
      return { playSecond };
    },
    template: '<button type="button" aria-label="Play Second" @click="playSecond">Library browser</button>',
  }),
}));

vi.mock("./components/NeteaseSource.vue", () => ({
  default: {
    name: "NeteaseSource",
    template: '<div data-testid="netease-source">NetEase source</div>',
  },
}));

describe("application shell", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    tauriMocks.listen.mockResolvedValue(vi.fn());
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "get_scan_status") {
        return Promise.resolve({
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
        });
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
    vi.restoreAllMocks();
  });

  it("navigates between all sidebar sections and closes the mobile drawer", async () => {
    const wrapper = mount(App);
    await flushPromises();

    const navigation = wrapper.get('nav[aria-label="Primary navigation"]');
    expect(navigation.findAll("button").map((button) => button.text())).toEqual([
      "Local Music",
      "Audio Sources",
      "Plugins",
      "Settings",
    ]);
    expect(wrapper.get("h1").text()).toBe("Local Music");
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
    expect(wrapper.find("#theme-preference").exists()).toBe(true);
    expect(wrapper.get('footer[aria-label="Playback bar"]').element).toBe(playbackBar.element);
    expect(settingsButton?.attributes("aria-current")).toBe("page");
    expect(drawerToggle.element.checked).toBe(false);
    wrapper.unmount();
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

  it("navigates local tracks and wraps at the end in repeat mode", async () => {
    tauriMocks.invoke.mockImplementation((command: string, payload?: { trackId?: number; index?: number }) => {
      if (command === "get_scan_status") {
        return Promise.resolve({
          isRunning: false,
          folderPath: "/music",
          discoveredFiles: 2,
          scannedFiles: 2,
          indexedTracks: 2,
          skippedFiles: 0,
          errorCount: 0,
          lastError: null,
          startedAt: null,
          finishedAt: null,
        });
      }
      if (command === "local_track_media_source") {
        return Promise.resolve({
          filePath: payload?.trackId === 2 ? "/music/second.mp3" : "/music/first.mp3",
          mimeType: "audio/mpeg",
        });
      }
      if (command === "local_library_queue_track") {
        return Promise.resolve({
          index: payload?.index ?? 0,
          track: {
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
          },
        });
      }
      if (command === "local_track_playback_details") {
        return Promise.resolve({ coverDataUrl: null, lyrics: null, lyricsError: null });
      }
      return Promise.resolve(null);
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
    expect(tauriMocks.invoke).toHaveBeenLastCalledWith("local_track_playback_details", {
      trackId: 1,
    });
    expect(wrapper.get('button[aria-label="Previous track"]').attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });
});
