import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import KugouSource from "./KugouSource.vue";
import type { SourcePlaylist } from "../lib/plugin-api";
import {
  createKugouTrack,
  createPluginRecord,
  createSourceAccount,
} from "../test/fixtures";
import { createTestQueryPlugin } from "../test/query-client";

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
vi.mock("@tanstack/vue-virtual", () => import("../test/vue-virtual.mock"));

const accountRef = "kugou-account:00000000-0000-4000-8000-000000000001";
const track = createKugouTrack({ coverUrl: "https://example.test/cover.jpg" });
const playlistCoverUrl = "https://example.test/playlist.jpg";
const playlist: SourcePlaylist = {
  id: "collection_3_42_1_0",
  name: "Daily",
  description: null,
  coverUrl: playlistCoverUrl,
  trackCount: 1,
  ownerName: "Fika",
  canMutate: false,
};

function kugouPluginRecord() {
  return createPluginRecord({
    id: "fika.kugou",
    name: "KuGou Music",
    description: null,
    path: "/plugins/kugou",
    state: "enabled",
    enabled: true,
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
  });
}

function mountKugouSource() {
  return mount(KugouSource, {
    props: {
      streamQuality: "320k",
      playbackSource: "source-one",
      audioSources: [{ value: "source-one", label: "Source One" }],
    },
    global: {
      plugins: [createTestQueryPlugin()],
    },
  });
}

describe("KugouSource", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    pluginApiMocks.cancelSourceRequest.mockResolvedValue(true);
    pluginApiMocks.listPlugins.mockResolvedValue([kugouPluginRecord()]);
    kugouApiMocks.listKugouAccounts.mockResolvedValue([
      createSourceAccount({ accountRef }),
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
      diagnostics: [],
    });
    kugouApiMocks.resolveKugouTrack.mockResolvedValue({
      track,
      url: "https://fsandroid.kugou.com/track.mp3",
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

  it("renders the Playlist cover in the KuGou Playlist navigation", async () => {
    const wrapper = mountKugouSource();
    await flushPromises();

    const playlistsTab = wrapper
      .findAll('[role="tab"]')
      .find((tab) => tab.text().trim() === "Playlists");
    await playlistsTab?.trigger("click");
    await flushPromises();

    expect(wrapper.find("aside img").attributes("src")).toBe(playlistCoverUrl);
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
