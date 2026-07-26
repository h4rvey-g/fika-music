import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NeteaseSource from "./NeteaseSource.vue";
import type { PluginRecord } from "../lib/plugin-api";
import { createPluginRecord, createSourceAccount } from "../test/fixtures";
import { createTestQueryPlugin } from "../test/query-client";

const pluginApiMocks = vi.hoisted(() => ({
  cancelSourceRequest: vi.fn(),
  listPlugins: vi.fn(),
}));

const neteaseApiMocks = vi.hoisted(() => ({
  cancelNeteaseQrLogin: vi.fn(),
  disconnectNeteaseAccount: vi.fn(),
  getNeteasePlaylist: vi.fn(),
  getNeteasePlaylists: vi.fn(),
  getNeteaseRecommendations: vi.fn(),
  listNeteaseAccounts: vi.fn(),
  listNeteaseMutationAudit: vi.fn(),
  pollNeteaseQrLogin: vi.fn(),
  startNeteaseQrLogin: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => pluginApiMocks);
vi.mock("../lib/netease-api", () => ({
  NETEASE_PLUGIN_ID: "fika.netease",
  ...neteaseApiMocks,
}));

const accountRef = "netease-account:00000000-0000-4000-8000-000000000001";

function neteasePluginRecord(overrides: Partial<PluginRecord> = {}): PluginRecord {
  return createPluginRecord({
    id: "fika.netease",
    name: "NetEase Cloud Music",
    description: null,
    path: "/plugins/netease",
    state: "enabled",
    enabled: true,
    declaredCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    grantedCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    requiredHostBridges: ["netease-api-enhanced"],
    providers: [],
    ...overrides,
  });
}

function mountNeteaseSource(
  props: Partial<{
    playbackSource: string;
    audioSources: Array<{ value: string; label: string }>;
  }> = {},
) {
  return mount(NeteaseSource, {
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

describe("NeteaseSource", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    pluginApiMocks.listPlugins.mockResolvedValue([neteasePluginRecord()]);
    neteaseApiMocks.listNeteaseAccounts.mockResolvedValue([
      createSourceAccount({ accountRef }),
    ]);
    neteaseApiMocks.cancelNeteaseQrLogin.mockResolvedValue(undefined);
  });

  it("shows login and audio source controls without loading music content", async () => {
    const wrapper = mountNeteaseSource();
    await flushPromises();

    expect(wrapper.text()).toContain("Fika · active");
    expect(wrapper.find('select[aria-label="NetEase playback source"]').exists()).toBe(true);
    expect(wrapper.find('select[aria-label="NetEase account"]').exists()).toBe(true);
    expect(wrapper.findAll('[role="tab"]')).toHaveLength(0);
    expect(wrapper.text()).not.toContain("Recommendations");
    expect(wrapper.text()).not.toContain("Playlists");
    expect(wrapper.text()).not.toContain("Audit");
    expect(neteaseApiMocks.getNeteaseRecommendations).not.toHaveBeenCalled();
    expect(neteaseApiMocks.getNeteasePlaylists).not.toHaveBeenCalled();
    expect(neteaseApiMocks.getNeteasePlaylist).not.toHaveBeenCalled();
    expect(neteaseApiMocks.listNeteaseMutationAudit).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("shares audio source changes with the application", async () => {
    const wrapper = mountNeteaseSource();
    await flushPromises();

    await wrapper
      .get<HTMLSelectElement>('select[aria-label="NetEase playback source"]')
      .setValue("source-two");

    expect(wrapper.emitted("update:playbackSource")?.[0]).toEqual(["source-two"]);
    wrapper.unmount();
  });

  it("opens audio source configuration when no source is enabled", async () => {
    const wrapper = mountNeteaseSource({ playbackSource: "", audioSources: [] });
    await flushPromises();

    expect(wrapper.text()).toContain("No enabled audio source is available");
    const openSources = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Open Audio Sources"));
    await openSources?.trigger("click");
    expect(wrapper.emitted("openAudioSources")).toBeTruthy();
    wrapper.unmount();
  });

  it("routes disabled Plugin state to direct enablement", async () => {
    pluginApiMocks.listPlugins.mockResolvedValue([
      neteasePluginRecord({ state: "disabled", enabled: false }),
    ]);
    const wrapper = mountNeteaseSource();
    await flushPromises();

    expect(wrapper.text()).toContain("Plugin is disabled");
    const openPlugins = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Open Plugins"));
    await openPlugins?.trigger("click");
    expect(wrapper.emitted("openPlugins")).toBeTruthy();
    wrapper.unmount();
  });

  it("cancels the host QR session when the connection view is dismissed", async () => {
    neteaseApiMocks.startNeteaseQrLogin.mockResolvedValue({
      sessionId: "qr-session",
      qrImageDataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
      expiresAt: 300,
    });
    const wrapper = mountNeteaseSource();
    await flushPromises();

    const connect = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Connect");
    await connect?.trigger("click");
    await flushPromises();
    const cancel = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Cancel");
    await cancel?.trigger("click");

    expect(neteaseApiMocks.cancelNeteaseQrLogin).toHaveBeenCalledWith("qr-session");
    wrapper.unmount();
  });
});
