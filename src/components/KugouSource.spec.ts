import { flushPromises, mount } from "@vue/test-utils";
import { QueryClient, VueQueryPlugin } from "@tanstack/vue-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import KugouSource from "./KugouSource.vue";
import type { PluginRecord, RemoteTrack, SourcePlaylist } from "../lib/plugin-api";

const pluginApiMocks = vi.hoisted(() => ({
  cancelSourceRequest: vi.fn(),
  listPlugins: vi.fn(),
}));

const audioSourceApiMocks = vi.hoisted(() => ({
  audioSourceLabel: vi.fn(),
  resolveAudioSourceTrack: vi.fn(),
}));

const kugouApiMocks = vi.hoisted(() => ({
  cancelKugouQrLogin: vi.fn(),
  disconnectKugouAccount: vi.fn(),
  getKugouPlaylist: vi.fn(),
  getKugouPlaylists: vi.fn(),
  getKugouRecommendations: vi.fn(),
  listKugouAccounts: vi.fn(),
  pollKugouQrLogin: vi.fn(),
  resolveKugouTrack: vi.fn(),
  startKugouQrLogin: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => pluginApiMocks);
vi.mock("../lib/audio-source-api", () => audioSourceApiMocks);
vi.mock("../lib/kugou-api", () => ({
  KUGOU_PLUGIN_ID: "fika.kugou",
  ...kugouApiMocks,
}));
vi.mock("@tanstack/vue-virtual", () => ({
  useVirtualizer: (options: { value: { count: number; estimateSize: () => number } }) =>
    ref({
      getVirtualItems: () =>
        Array.from({ length: Math.min(options.value.count, 20) }, (_, index) => ({
          index,
          key: index,
          start: index * options.value.estimateSize(),
          size: options.value.estimateSize(),
          end: (index + 1) * options.value.estimateSize(),
          lane: 0,
        })),
      getTotalSize: () => options.value.count * options.value.estimateSize(),
      measure: vi.fn(),
      scrollToIndex: vi.fn(),
    }),
}));

const accountRef = "kugou-account:00000000-0000-4000-8000-000000000001";
const track: RemoteTrack = {
  id: "4D766DEC7A90A011D730ED939D158131",
  source: "kg",
  title: "Under My Skin",
  artist: "Andrew Cui",
  album: "Under My Skin",
  durationSeconds: 205,
  coverUrl: "https://example.test/cover.jpg",
  rawInfo: { hash: "4D766DEC7A90A011D730ED939D158131" },
};
const playlist: SourcePlaylist = {
  id: "collection_3_42_1_0",
  name: "Daily",
  description: null,
  coverUrl: null,
  trackCount: 1,
  ownerName: "Fika",
  canMutate: false,
};

function pluginRecord(): PluginRecord {
  return {
    id: "fika.kugou",
    name: "KuGou Music",
    version: "0.1.0",
    description: null,
    author: "Fika Music",
    path: "/plugins/kugou",
    origin: "bundled",
    state: "enabled",
    enabled: true,
    permissionsReviewed: true,
    declaredCapabilities: [
      "account:ref",
      "playlist:read",
      "bridge:kugou-music-api",
    ],
    grantedCapabilities: [
      "account:ref",
      "playlist:read",
      "bridge:kugou-music-api",
    ],
    requiredHostBridges: ["kugou-music-api"],
    providers: [],
    diagnostics: [],
    canRemove: false,
    canEnable: true,
    manifest: null,
  };
}

function mountKugouSource() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return mount(KugouSource, {
    props: {
      streamQuality: "320k",
      playbackSource: "source-one",
      audioSources: [{ value: "source-one", label: "Source One" }],
    },
    global: {
      plugins: [[VueQueryPlugin, { queryClient }]],
    },
  });
}

describe("KugouSource", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    pluginApiMocks.cancelSourceRequest.mockResolvedValue(true);
    pluginApiMocks.listPlugins.mockResolvedValue([pluginRecord()]);
    kugouApiMocks.listKugouAccounts.mockResolvedValue([
      {
        accountRef,
        userId: "42",
        displayName: "Fika",
        avatarUrl: null,
        status: "active",
        connectedAt: 1,
        lastVerifiedAt: 1,
      },
    ]);
    kugouApiMocks.getKugouRecommendations.mockResolvedValue({
      data: [track],
      diagnostics: [],
    });
    kugouApiMocks.getKugouPlaylists.mockResolvedValue({
      data: [playlist],
      diagnostics: [],
    });
    kugouApiMocks.getKugouPlaylist.mockResolvedValue({
      data: { playlist, tracks: [track] },
      diagnostics: [],
    });
    kugouApiMocks.cancelKugouQrLogin.mockResolvedValue(undefined);
    audioSourceApiMocks.audioSourceLabel.mockReturnValue("Source One");
    audioSourceApiMocks.resolveAudioSourceTrack.mockResolvedValue({
      url: "https://cdn.example.test/track.mp3",
      mimeType: "audio/mpeg",
      diagnostics: [],
    });
    kugouApiMocks.resolveKugouTrack.mockResolvedValue({
      track,
      url: "https://fsandroid.kugou.com/track.mp3",
      mimeType: "audio/mpeg",
      providerName: "KuGou Music",
      diagnostics: [],
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("loads recommendations and resolves playback through the selected audio source", async () => {
    const wrapper = mountKugouSource();
    await flushPromises();

    await wrapper.get('button[aria-label="Play Under My Skin"]').trigger("click");
    await flushPromises();

    expect(audioSourceApiMocks.resolveAudioSourceTrack).toHaveBeenCalledWith(
      expect.objectContaining({
        audioSourceId: "source-one",
        source: "kg",
        trackId: track.id,
        musicInfo: {
          hash: track.id,
          name: track.title,
          singer: track.artist,
          artist: track.artist,
          album: track.album,
        },
        quality: "320k",
      }),
    );
    expect(kugouApiMocks.resolveKugouTrack).not.toHaveBeenCalled();
    expect(wrapper.emitted("playbackReady")?.[0]?.[0]).toMatchObject({
      track,
      providerName: "Source One",
    });
    wrapper.unmount();
  });

  it("falls back to the authenticated KuGou provider when the audio source fails", async () => {
    audioSourceApiMocks.resolveAudioSourceTrack.mockRejectedValueOnce(
      new Error(
        "all static-templates music URL candidates failed: aggregate endpoint returned HTTP 400",
      ),
    );
    const wrapper = mountKugouSource();
    await flushPromises();

    await wrapper.get('button[aria-label="Play Under My Skin"]').trigger("click");
    await flushPromises();

    expect(kugouApiMocks.resolveKugouTrack).toHaveBeenCalledWith(
      track,
      "320k",
      accountRef,
      expect.any(String),
    );
    expect(wrapper.emitted("playbackReady")?.[0]?.[0]).toMatchObject({
      track,
      url: "https://fsandroid.kugou.com/track.mp3",
      providerName: "KuGou Music",
    });
    expect(wrapper.text()).toContain("Audio Source failed; using KuGou Music");
    wrapper.unmount();
  });

  it("loads a selected Playlist as a read-only track list", async () => {
    const wrapper = mountKugouSource();
    await flushPromises();

    const playlistsTab = wrapper
      .findAll('[role="tab"]')
      .find((tab) => tab.text().trim() === "Playlists");
    await playlistsTab?.trigger("click");
    await flushPromises();

    expect(kugouApiMocks.getKugouPlaylist).toHaveBeenCalledWith(
      accountRef,
      playlist.id,
      expect.any(String),
    );
    expect(wrapper.text()).toContain("Read only");
    expect(wrapper.find('button[aria-label="Play Under My Skin"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("cancels the host QR session when the connection view is dismissed", async () => {
    kugouApiMocks.startKugouQrLogin.mockResolvedValue({
      sessionId: "qr-session",
      qrImageDataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
      expiresAt: 300,
    });
    const wrapper = mountKugouSource();
    await flushPromises();

    const connect = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Connect");
    await connect?.trigger("click");
    await flushPromises();
    await wrapper.get('img[alt="KuGou login QR code"]').trigger("load");

    wrapper.unmount();

    expect(kugouApiMocks.cancelKugouQrLogin).toHaveBeenCalledWith("qr-session");
  });
});
