import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NeteaseSource from "./NeteaseSource.vue";
import type { PluginRecord, RemoteTrack, SourcePlaylist } from "../lib/plugin-api";

const pluginApiMocks = vi.hoisted(() => ({
  listPlugins: vi.fn(),
}));

const neteaseApiMocks = vi.hoisted(() => ({
  addNeteasePlaylistTrack: vi.fn(),
  cancelNeteaseQrLogin: vi.fn(),
  disconnectNeteaseAccount: vi.fn(),
  getNeteasePlaylist: vi.fn(),
  getNeteasePlaylists: vi.fn(),
  getNeteaseRecommendations: vi.fn(),
  listNeteaseAccounts: vi.fn(),
  listNeteaseMutationAudit: vi.fn(),
  pollNeteaseQrLogin: vi.fn(),
  removeNeteasePlaylistTrack: vi.fn(),
  resolveNeteaseTrack: vi.fn(),
  startNeteaseQrLogin: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => pluginApiMocks);
vi.mock("../lib/netease-api", () => ({
  NETEASE_PLUGIN_ID: "fika.netease",
  ...neteaseApiMocks,
}));

const accountRef = "netease-account:00000000-0000-4000-8000-000000000001";

const track: RemoteTrack = {
  id: "347230",
  source: "wy",
  title: "Test Track",
  artist: "Test Artist",
  album: "Test Album",
  durationSeconds: 180,
  coverUrl: null,
  rawInfo: { id: 347230 },
};

const playlist: SourcePlaylist = {
  id: "playlist-1",
  name: "My Playlist",
  description: null,
  coverUrl: null,
  trackCount: 0,
  ownerName: "Fika",
  canMutate: true,
};

function pluginRecord(overrides: Partial<PluginRecord> = {}): PluginRecord {
  return {
    id: "fika.netease",
    name: "NetEase Cloud Music",
    version: "0.1.0",
    description: null,
    author: "Fika Music",
    path: "/plugins/netease",
    origin: "bundled",
    state: "enabled",
    enabled: true,
    permissionsReviewed: true,
    declaredCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    grantedCapabilities: ["account:ref", "playlist:read", "playlist:write"],
    requiredHostBridges: ["netease-api-enhanced"],
    providers: [],
    diagnostics: [],
    canRemove: false,
    canEnable: true,
    ...overrides,
  };
}

describe("NeteaseSource", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    pluginApiMocks.listPlugins.mockResolvedValue([pluginRecord()]);
    neteaseApiMocks.listNeteaseAccounts.mockResolvedValue([
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
    neteaseApiMocks.getNeteaseRecommendations.mockResolvedValue({
      data: [track],
      diagnostics: [],
    });
    neteaseApiMocks.getNeteasePlaylists.mockResolvedValue({
      data: [playlist],
      diagnostics: [],
    });
    neteaseApiMocks.getNeteasePlaylist.mockResolvedValue({
      data: { playlist, tracks: [] },
      diagnostics: [],
    });
    neteaseApiMocks.listNeteaseMutationAudit.mockResolvedValue([]);
    neteaseApiMocks.addNeteasePlaylistTrack.mockResolvedValue({
      data: {
        auditId: 1,
        operation: "add",
        playlistId: playlist.id,
        trackId: track.id,
        occurredAt: 1,
      },
      diagnostics: [],
    });
  });

  it("loads normalized recommendations for an active Account Ref", async () => {
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });

    await flushPromises();

    expect(neteaseApiMocks.getNeteaseRecommendations).toHaveBeenCalledWith(
      accountRef,
      expect.any(String),
    );
    expect(wrapper.text()).toContain("Test Track");
    wrapper.unmount();
  });

  it("keeps recommendations available when Playlist permission is denied", async () => {
    neteaseApiMocks.getNeteasePlaylists.mockRejectedValue({
      message: "playlist:read capability is not granted",
    });
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });

    await flushPromises();

    expect(wrapper.text()).toContain("Test Track");
    expect(wrapper.text()).toContain("Playlists: playlist:read capability is not granted");
    wrapper.unmount();
  });

  it("refreshes the account state after credential expiry", async () => {
    const activeAccount = {
      accountRef,
      userId: "42",
      displayName: "Fika",
      avatarUrl: null,
      status: "active",
      connectedAt: 1,
      lastVerifiedAt: 1,
    };
    neteaseApiMocks.listNeteaseAccounts
      .mockResolvedValueOnce([activeAccount])
      .mockResolvedValue([{ ...activeAccount, status: "expired" }]);
    neteaseApiMocks.getNeteaseRecommendations.mockRejectedValue({
      message: "NetEase account session expired; reconnect the account",
    });
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });

    await flushPromises();

    expect(wrapper.text()).toContain("Fika · expired");
    expect(wrapper.text()).toContain("reconnect the account");
    wrapper.unmount();
  });

  it("routes disabled Plugin state to permission review", async () => {
    pluginApiMocks.listPlugins.mockResolvedValue([
      pluginRecord({ state: "needs-review", enabled: false }),
    ]);
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });

    await flushPromises();

    expect(wrapper.text()).toContain("Plugin permission review required");
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
    neteaseApiMocks.cancelNeteaseQrLogin.mockResolvedValue(undefined);
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });
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

  it("requires confirmation before adding a recommendation to a Playlist", async () => {
    const wrapper = mount(NeteaseSource, { props: { streamQuality: "320k" } });
    await flushPromises();

    await wrapper.get('button[aria-label="Add Test Track to a Playlist"]').trigger("click");
    expect(wrapper.get('[role="dialog"]').text()).toContain("Add to Playlist");
    const confirm = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Confirm"));
    await confirm?.trigger("click");
    await flushPromises();

    expect(neteaseApiMocks.addNeteasePlaylistTrack).toHaveBeenCalledWith(
      accountRef,
      "playlist-1",
      track,
    );
    wrapper.unmount();
  });
});
