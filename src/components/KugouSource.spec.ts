import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import KugouSource from "./KugouSource.vue";
import { createPluginRecord, createSourceAccount } from "../test/fixtures";
import { createTestQueryPlugin } from "../test/query-client";

const pluginApiMocks = vi.hoisted(() => ({
  cancelSourceRequest: vi.fn(),
  listPlugins: vi.fn(),
}));

const kugouApiMocks = vi.hoisted(() => ({
  cancelKugouQrLogin: vi.fn(),
  disconnectKugouAccount: vi.fn(),
  getKugouPlaylist: vi.fn(),
  getKugouPlaylists: vi.fn(),
  getKugouRecommendations: vi.fn(),
  listKugouAccounts: vi.fn(),
  pollKugouQrLogin: vi.fn(),
  startKugouQrLogin: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => pluginApiMocks);
vi.mock("../lib/kugou-api", () => ({
  KUGOU_PLUGIN_ID: "fika.kugou",
  ...kugouApiMocks,
}));

const accountRef = "kugou-account:00000000-0000-4000-8000-000000000001";

function kugouPluginRecord() {
  return createPluginRecord({
    id: "fika.kugou",
    name: "KuGou Music",
    description: null,
    path: "/plugins/kugou",
    state: "enabled",
    enabled: true,
    declaredCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    grantedCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    requiredHostBridges: ["kugou-music-api"],
    providers: [],
  });
}

function mountKugouSource(
  props: Partial<{
    playbackSource: string;
    audioSources: Array<{ value: string; label: string }>;
  }> = {},
) {
  return mount(KugouSource, {
    props: {
      playbackSource: "source-one",
      audioSources: [
        { value: "source-one", label: "Source One" },
        { value: "source-two", label: "Source Two" },
      ],
      ...props,
    },
    global: {
      plugins: [createTestQueryPlugin()],
    },
  });
}

describe("KugouSource", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    pluginApiMocks.listPlugins.mockResolvedValue([kugouPluginRecord()]);
    kugouApiMocks.listKugouAccounts.mockResolvedValue([
      createSourceAccount({ accountRef }),
    ]);
    kugouApiMocks.cancelKugouQrLogin.mockResolvedValue(undefined);
  });

  it("shows login and audio source controls without loading music content", async () => {
    const wrapper = mountKugouSource();
    await flushPromises();

    expect(wrapper.text()).toContain("Fika · active");
    expect(wrapper.find('select[aria-label="KuGou playback source"]').exists()).toBe(true);
    expect(wrapper.find('select[aria-label="KuGou account"]').exists()).toBe(true);
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(0);
    expect(wrapper.text()).not.toContain("Recommendations");
    expect(wrapper.text()).not.toContain("Playlists");
    expect(kugouApiMocks.getKugouRecommendations).not.toHaveBeenCalled();
    expect(kugouApiMocks.getKugouPlaylists).not.toHaveBeenCalled();
    expect(kugouApiMocks.getKugouPlaylist).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("shares audio source changes with the application", async () => {
    const wrapper = mountKugouSource();
    await flushPromises();

    await wrapper
      .get<HTMLSelectElement>('select[aria-label="KuGou playback source"]')
      .setValue("source-two");

    expect(wrapper.emitted("update:playbackSource")?.[0]).toEqual(["source-two"]);
    wrapper.unmount();
  });

  it("opens audio source configuration when no source is enabled", async () => {
    const wrapper = mountKugouSource({ playbackSource: "", audioSources: [] });
    await flushPromises();

    expect(wrapper.text()).toContain("No enabled audio source is available");
    const openSources = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Open Audio Sources"));
    await openSources?.trigger("click");
    expect(wrapper.emitted("openAudioSources")).toBeTruthy();
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
    wrapper.unmount();

    expect(kugouApiMocks.cancelKugouQrLogin).toHaveBeenCalledWith("qr-session");
  });
});
