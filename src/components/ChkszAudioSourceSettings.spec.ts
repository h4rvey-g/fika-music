import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ChkszAudioSourceSettings from "./ChkszAudioSourceSettings.vue";

const api = vi.hoisted(() => ({
  clearChkszApiKey: vi.fn(),
  getChkszApiKeyStatus: vi.fn(),
  setChkszApiKey: vi.fn(),
}));

vi.mock("../lib/chksz-audio-source-api", () => api);

describe("ChkszAudioSourceSettings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getChkszApiKeyStatus.mockResolvedValue(true);
    api.setChkszApiKey.mockResolvedValue(undefined);
    api.clearChkszApiKey.mockResolvedValue(undefined);
  });

  it("reports configuration without reading the saved key", async () => {
    const wrapper = mount(ChkszAudioSourceSettings);
    await flushPromises();

    expect(wrapper.text()).toContain("Configured");
    expect(wrapper.get<HTMLInputElement>('input[aria-label="ChKSz API key"]').element.value)
      .toBe("");
  });

  it("saves a replacement key and clears the input", async () => {
    const wrapper = mount(ChkszAudioSourceSettings);
    await flushPromises();

    const input = wrapper.get<HTMLInputElement>('input[aria-label="ChKSz API key"]');
    await input.setValue("replacement-key");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(api.setChkszApiKey).toHaveBeenCalledWith("replacement-key");
    expect(input.element.value).toBe("");
    expect(wrapper.text()).toContain("ChKSz API key saved.");
  });

  it("clears the configured key after confirmation", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const wrapper = mount(ChkszAudioSourceSettings);
    await flushPromises();

    const clear = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Clear API key"));
    await clear?.trigger("click");
    await flushPromises();

    expect(api.clearChkszApiKey).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("ChKSz API key cleared.");
  });
});
