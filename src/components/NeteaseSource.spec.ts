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
  cancelNeteasePasswordLogin: vi.fn(),
  cancelNeteasePhoneLogin: vi.fn(),
  cancelNeteaseQrLogin: vi.fn(),
  completeNeteasePasswordLogin: vi.fn(),
  completeNeteasePhoneLogin: vi.fn(),
  disconnectNeteaseAccount: vi.fn(),
  getNeteasePlaylist: vi.fn(),
  getNeteasePlaylists: vi.fn(),
  getNeteaseRecommendations: vi.fn(),
  listNeteaseAccounts: vi.fn(),
  listNeteaseMutationAudit: vi.fn(),
  loginNeteasePassword: vi.fn(),
  pollNeteaseQrLogin: vi.fn(),
  startNeteasePhoneLogin: vi.fn(),
  startNeteaseQrLogin: vi.fn(),
  neteaseWebLoginSupported: vi.fn(),
  startNeteaseWebLogin: vi.fn(),
  pollNeteaseWebLogin: vi.fn(),
  cancelNeteaseWebLogin: vi.fn(),
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
    neteaseApiMocks.cancelNeteasePasswordLogin.mockResolvedValue(undefined);
    neteaseApiMocks.cancelNeteaseQrLogin.mockResolvedValue(undefined);
    neteaseApiMocks.cancelNeteasePhoneLogin.mockResolvedValue(undefined);
    neteaseApiMocks.neteaseWebLoginSupported.mockResolvedValue(false);
    neteaseApiMocks.cancelNeteaseWebLogin.mockResolvedValue(undefined);
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
    const qrTab = wrapper
      .findAll('[role="tab"]')
      .find((tab) => tab.text().includes("QR code"));
    await qrTab?.trigger("click");
    const createQr = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Create QR code");
    await createQr?.trigger("click");
    await flushPromises();
    const cancel = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Cancel QR code");
    await cancel?.trigger("click");

    expect(neteaseApiMocks.cancelNeteaseQrLogin).toHaveBeenCalledWith("qr-session");
    wrapper.unmount();
  });

  it.each([
    { code: "api-failure", message: "NetEase API rejected poll QR login (code -462): 请完成验证操作" },
    { code: "verification-required", message: "NetEase API rejected poll QR login (code 8821): verification required" },
  ])("pauses a risk-controlled QR poll without hiding the code or retrying automatically: $code", async (error) => {
    neteaseApiMocks.startNeteaseQrLogin.mockResolvedValue({
      sessionId: "qr-session",
      qrImageDataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
      expiresAt: 300,
    });
    neteaseApiMocks.pollNeteaseQrLogin.mockRejectedValue(error);
    const wrapper = mountNeteaseSource();
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text().trim() === "Connect")?.trigger("click");
    await wrapper.findAll('[role="tab"]').find((tab) => tab.text().includes("QR code"))?.trigger("click");
    await wrapper.findAll("button").find((button) => button.text().trim() === "Create QR code")?.trigger("click");
    await flushPromises();

    await vi.waitFor(() => {
      expect(wrapper.text()).toContain("QR login paused for security verification");
    }, { timeout: 3_000 });
    expect(wrapper.find('img[alt="NetEase login QR code"]').exists()).toBe(true);
    expect(wrapper.text()).toContain(error.message);
    expect(neteaseApiMocks.pollNeteaseQrLogin).toHaveBeenCalledTimes(1);
    expect(neteaseApiMocks.cancelNeteaseQrLogin).not.toHaveBeenCalled();

    await wrapper.findAll("button").find((button) => button.text() === "Check QR login again")?.trigger("click");
    await flushPromises();
    expect(neteaseApiMocks.pollNeteaseQrLogin).toHaveBeenCalledTimes(2);
    wrapper.unmount();
  });

  it("connects with an SMS verification code on one device", async () => {
    const phoneAccount = createSourceAccount({
      accountRef: "netease-account:00000000-0000-4000-8000-000000000002",
      displayName: "Phone User",
    });
    neteaseApiMocks.listNeteaseAccounts
      .mockResolvedValueOnce([])
      .mockResolvedValue([phoneAccount]);
    neteaseApiMocks.startNeteasePhoneLogin.mockResolvedValue({
      sessionId: "phone-session",
      expiresAt: 600,
    });
    neteaseApiMocks.completeNeteasePhoneLogin.mockResolvedValue(phoneAccount);
    const wrapper = mountNeteaseSource();
    await flushPromises();

    const connect = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Connect");
    await connect?.trigger("click");
    await wrapper.get('input[aria-label="NetEase phone number"]').setValue("13800138000");
    const sendCode = wrapper
      .findAll("button")
      .find((button) => button.text().trim() === "Send code");
    await sendCode?.trigger("click");
    await flushPromises();
    await wrapper.get('input[aria-label="NetEase verification code"]').setValue("123456");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(neteaseApiMocks.startNeteasePhoneLogin).toHaveBeenCalledWith("13800138000");
    expect(neteaseApiMocks.completeNeteasePhoneLogin).toHaveBeenCalledWith(
      "phone-session",
      "123456",
    );
    expect(wrapper.text()).toContain("Phone User connected.");
    wrapper.unmount();
  });

  it("defaults to official website login on desktop without collecting credentials", async () => {
    const webAccount = createSourceAccount({
      accountRef: "netease-account:00000000-0000-4000-8000-000000000003",
      displayName: "Web User",
    });
    neteaseApiMocks.neteaseWebLoginSupported.mockResolvedValue(true);
    neteaseApiMocks.listNeteaseAccounts
      .mockResolvedValueOnce([])
      .mockResolvedValue([webAccount]);
    neteaseApiMocks.startNeteaseWebLogin.mockResolvedValue({ sessionId: "web-session", expiresAt: 600 });
    neteaseApiMocks.pollNeteaseWebLogin.mockResolvedValue({ status: "connected", account: webAccount });
    const wrapper = mountNeteaseSource();
    await flushPromises();

    await wrapper.findAll("button").find((button) => button.text().trim() === "Connect")?.trigger("click");
    expect(wrapper.get('[role="tab"][aria-selected="true"]').text()).toContain("Official website");
    expect(wrapper.find('input[type="password"]').exists()).toBe(false);
    await wrapper.findAll("button").find((button) => button.text().trim() === "Sign in on official website")?.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("Waiting for official website login");
    await vi.waitFor(() => expect(wrapper.text()).toContain("Web User connected."), { timeout: 3_000 });
    expect(neteaseApiMocks.pollNeteaseWebLogin).toHaveBeenCalledWith("web-session");
    expect(neteaseApiMocks.loginNeteasePassword).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("closes the official login window when switching login methods", async () => {
    neteaseApiMocks.neteaseWebLoginSupported.mockResolvedValue(true);
    neteaseApiMocks.startNeteaseWebLogin.mockResolvedValue({ sessionId: "web-session", expiresAt: 600 });
    const wrapper = mountNeteaseSource();
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text().trim() === "Connect")?.trigger("click");
    await wrapper.findAll("button").find((button) => button.text().trim() === "Sign in on official website")?.trigger("click");
    await flushPromises();
    await wrapper.findAll('[role="tab"]').find((tab) => tab.text().includes("QR code"))?.trigger("click");
    await flushPromises();
    expect(neteaseApiMocks.cancelNeteaseWebLogin).toHaveBeenCalledWith("web-session");
    expect(wrapper.text()).not.toContain("Waiting for official website login");
    wrapper.unmount();
  });

  it("does not expose official website login on unsupported platforms", async () => {
    const wrapper = mountNeteaseSource();
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text().trim() === "Connect")?.trigger("click");
    expect(wrapper.text()).not.toContain("Official website");
    expect(wrapper.get('[role="tab"][aria-selected="true"]').text()).toContain("Verification code");
    wrapper.unmount();
  });

  it("opens a security-verification dialog when sending an SMS requires verification", async () => {
    neteaseApiMocks.startNeteasePhoneLogin.mockRejectedValue({
      code: "api-failure",
      message: "NetEase API rejected send verification code (code -462): 请完成验证操作",
    });
    const wrapper = mountNeteaseSource();
    await flushPromises();
    await wrapper.findAll("button").find((button) => button.text().trim() === "Connect")?.trigger("click");
    await wrapper.get('input[aria-label="NetEase phone number"]').setValue("13800138000");
    await wrapper.findAll("button").find((button) => button.text().trim() === "Send code")?.trigger("click");
    await flushPromises();

    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.text()).toContain("NetEase security verification");
    expect(dialog.text()).toContain("No usable verification challenge was returned.");
    expect(wrapper.find('input[aria-label="NetEase verification code"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Verification code sent");
    await dialog.findAll("button").find((button) => button.text().includes("Use QR-code login"))?.trigger("click");
    expect(wrapper.get('[role="tab"][aria-selected="true"]').text()).toContain("QR code");
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    wrapper.unmount();
  });
});
