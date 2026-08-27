import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import OnlineMusicSettings from "./OnlineMusicSettings.vue";
import { createAudioSourceRecord, createOnlineMusicSettings } from "../test/fixtures";

const api = vi.hoisted(() => ({
  clearOnlineSearchHistory: vi.fn(),
  getOnlineMusicSettings: vi.fn(),
  listOnlineMusicChannels: vi.fn(),
  selectOnlineDownloadDirectory: vi.fn(),
  updateOnlineMusicSettings: vi.fn(),
}));

vi.mock("../lib/online-music-api", () => api);

describe("OnlineMusicSettings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getOnlineMusicSettings.mockResolvedValue(createOnlineMusicSettings());
    api.listOnlineMusicChannels.mockResolvedValue([]);
    api.updateOnlineMusicSettings.mockImplementation((settings) => Promise.resolve(settings));
  });

  it("hides manual source priority while automatic selection is active", async () => {
    const wrapper = mount(OnlineMusicSettings, {
      props: { audioSources: [createAudioSourceRecord()] },
    });
    await flushPromises();

    expect(wrapper.find('[data-testid="audio-source-priority"]').exists()).toBe(false);

    await wrapper.get('input[aria-label="Manual"]').setValue(true);
    await flushPromises();
    expect(wrapper.find('[data-testid="audio-source-priority"]').exists()).toBe(true);
    expect(api.updateOnlineMusicSettings).toHaveBeenCalledWith(
      expect.objectContaining({ audioSourceSelectionMode: "manual" }),
    );
  });

  it("persists independent playback and download quality settings", async () => {
    const wrapper = mount(OnlineMusicSettings, {
      props: { audioSources: [createAudioSourceRecord()] },
    });
    await flushPromises();

    await wrapper.get('[data-testid="online-playback-quality"]').setValue("flac");
    await flushPromises();
    expect(api.updateOnlineMusicSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ playbackQuality: "flac", downloadQuality: "320k" }),
    );

    await wrapper.get('[data-testid="online-download-quality"]').setValue("128k");
    await flushPromises();
    expect(api.updateOnlineMusicSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ playbackQuality: "flac", downloadQuality: "128k" }),
    );
  });

  it("persists the playback cache limit in megabytes", async () => {
    const wrapper = mount(OnlineMusicSettings, {
      props: { audioSources: [createAudioSourceRecord()] },
    });
    await flushPromises();

    await wrapper.get('[data-testid="online-playback-cache-limit"]').setValue("750");
    await flushPromises();

    expect(api.updateOnlineMusicSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ playbackCacheMaxMb: 750 }),
    );
  });
});
