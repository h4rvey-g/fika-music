import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import PluginManager from "./PluginManager.vue";
import type { PluginRecord } from "../lib/plugin-api";

const apiMocks = vi.hoisted(() => ({
  clearPluginDiagnostics: vi.fn(),
  installPluginPackage: vi.fn(),
  listPlugins: vi.fn(),
  refreshPluginRegistry: vi.fn(),
  removePluginPackage: vi.fn(),
  selectPluginPackage: vi.fn(),
  setPluginCapabilities: vi.fn(),
  setPluginEnabled: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => apiMocks);

function pluginRecord(overrides: Partial<PluginRecord> = {}): PluginRecord {
  return {
    id: "fika.runtime-demo",
    name: "Runtime Demo",
    version: "0.1.0",
    description: "Plugin manager integration fixture",
    author: "Fika Music",
    path: "/plugins/runtime-demo",
    origin: "bundled",
    state: "needs-review",
    enabled: false,
    permissionsReviewed: false,
    declaredCapabilities: ["network:any"],
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
    canEnable: false,
    manifest: null,
    ...overrides,
  };
}

describe("PluginManager", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMocks.listPlugins.mockResolvedValue([pluginRecord()]);
  });

  it("loads Plugin records through the typed API", async () => {
    const wrapper = mount(PluginManager);

    await flushPromises();

    expect(apiMocks.listPlugins).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("Runtime Demo");
    expect(wrapper.text()).toContain("Review required");
    expect(wrapper.emitted("pluginsChanged")?.[0]).toEqual([[pluginRecord()]]);
    wrapper.unmount();
  });

  it("reports enabled Plugin records to the application shell", async () => {
    const disabled = pluginRecord({
      state: "disabled",
      permissionsReviewed: true,
      canEnable: true,
    });
    const enabled = pluginRecord({
      state: "enabled",
      enabled: true,
      permissionsReviewed: true,
      canEnable: true,
    });
    apiMocks.listPlugins.mockResolvedValue([disabled]);
    apiMocks.setPluginEnabled.mockResolvedValue(enabled);
    const wrapper = mount(PluginManager);
    await flushPromises();

    const enableButton = wrapper.findAll("button").find((button) => button.text() === "Enable");
    expect(enableButton).toBeDefined();
    await enableButton?.trigger("click");
    await flushPromises();

    expect(apiMocks.setPluginEnabled).toHaveBeenCalledWith("fika.runtime-demo", true);
    const pluginChanges = wrapper.emitted("pluginsChanged") ?? [];
    expect(pluginChanges[pluginChanges.length - 1]).toEqual([[enabled]]);
    wrapper.unmount();
  });

  it("submits an explicit capability review", async () => {
    apiMocks.setPluginCapabilities.mockResolvedValue(
      pluginRecord({
        state: "disabled",
        permissionsReviewed: true,
        canEnable: true,
      }),
    );
    const wrapper = mount(PluginManager);
    await flushPromises();
    await wrapper
      .get('button[aria-label="Inspect Runtime Demo"]')
      .trigger("click");
    const confirmReview = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Confirm review"));

    expect(confirmReview).toBeDefined();
    await confirmReview?.trigger("click");
    await flushPromises();

    expect(apiMocks.setPluginCapabilities).toHaveBeenCalledWith(
      "fika.runtime-demo",
      [],
      true,
    );
    expect(wrapper.text()).toContain("Plugin permissions saved.");
    wrapper.unmount();
  });
});
