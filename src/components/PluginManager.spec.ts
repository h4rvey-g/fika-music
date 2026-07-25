import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import PluginManager from "./PluginManager.vue";
import type { PluginRecord } from "../lib/plugin-api";
import { createPluginRecord } from "../test/fixtures";

const apiMocks = vi.hoisted(() => ({
  clearPluginDiagnostics: vi.fn(),
  installPluginPackage: vi.fn(),
  listPlugins: vi.fn(),
  refreshPluginRegistry: vi.fn(),
  removePluginPackage: vi.fn(),
  selectPluginPackage: vi.fn(),
  setPluginEnabled: vi.fn(),
}));

vi.mock("../lib/plugin-api", () => apiMocks);

const pluginRecord = (overrides: Partial<PluginRecord> = {}) =>
  createPluginRecord({
    name: "Runtime Demo",
    description: "Plugin manager integration fixture",
    permissionsReviewed: false,
    declaredCapabilities: ["network:any"],
    ...overrides,
  });

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
    expect(wrapper.text()).toContain("Disabled");
    expect(wrapper.emitted("pluginsChanged")?.[0]).toEqual([[pluginRecord()]]);
    wrapper.unmount();
  });

  it("reports enabled Plugin records to the application shell", async () => {
    const disabled = pluginRecord();
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

  it("shows declared capabilities without review controls", async () => {
    const wrapper = mount(PluginManager);
    await flushPromises();
    await wrapper
      .get('button[aria-label="Inspect Runtime Demo"]')
      .trigger("click");

    expect(wrapper.text()).toContain("Network requests");
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Confirm review");
    wrapper.unmount();
  });
});
