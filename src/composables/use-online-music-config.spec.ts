import { QueryClient } from "@tanstack/vue-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createOnlineMusicConfig } from "./use-online-music-config";

const api = vi.hoisted(() => ({
  getOnlineMusicSettings: vi.fn(),
  invalidateOnlinePlaybackCaches: vi.fn(),
  listOnlineMusicChannels: vi.fn(),
}));

vi.mock("../lib/online-music-api", () => api);

const settings = {
  excludedChannels: [],
  channelPriority: [],
  audioSourcePriority: [],
  layerTimeoutSeconds: 8,
  playbackTimeoutSeconds: 20,
  preferredQuality: "320k" as const,
  searchHistoryEnabled: true,
  downloadDirectory: null,
  filenameTemplate: "{artist} - {title}",
  downloadConcurrency: 2,
  batchNotifications: true,
};

describe("online music config", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getOnlineMusicSettings.mockResolvedValue(settings);
    api.listOnlineMusicChannels.mockResolvedValue([]);
  });

  it("reuses settings and channels across track changes", async () => {
    const config = createOnlineMusicConfig(new QueryClient());

    await config.load();
    await config.load();

    expect(api.getOnlineMusicSettings).toHaveBeenCalledTimes(1);
    expect(api.listOnlineMusicChannels).toHaveBeenCalledTimes(1);
  });

  it("keeps updated settings and reloads derived channels", async () => {
    const config = createOnlineMusicConfig(new QueryClient());
    await config.load();
    const updated = { ...settings, excludedChannels: ["netease"] };

    config.updateSettings(updated);
    const result = await config.load();

    expect(result.settings).toEqual(updated);
    expect(api.getOnlineMusicSettings).toHaveBeenCalledTimes(1);
    expect(api.listOnlineMusicChannels).toHaveBeenCalledTimes(2);
    expect(api.invalidateOnlinePlaybackCaches).toHaveBeenCalledTimes(1);
  });
});
